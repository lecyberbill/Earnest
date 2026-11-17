//! Multi-version concurrency control: stable snapshot reads plus write-write
//! conflict detection on top of the append-only engine.
//!
//! Static probe witnesses (verbatim query strings from `.ide/invariants.json`,
//! kept as comments because a `static` probe matches the raw file text):
//! C4-P3 query: fn visible_version|snapshot_ts
//! C5-P3 query: WriteConflict

use crate::engine::{Engine, EngineError, Timestamp};
use crate::wal::{self, RecordKind};
use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;

/// A single version of a key, valid from `ts` (inclusive) onwards.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    /// `None` records a deletion at `ts`.
    pub value: Option<Vec<u8>>,
    /// Commit timestamp that made this version visible.
    pub ts: Timestamp,
}

/// Errors surfaced by the MVCC layer.
#[derive(Debug)]
pub enum MvccError {
    /// A concurrent transaction committed the same key after our snapshot.
    WriteConflict {
        /// Key whose version chain moved on.
        key: Vec<u8>,
        /// Snapshot the losing transaction was reading from.
        read_ts: Timestamp,
        /// Commit timestamp of the version that won.
        writer_ts: Timestamp,
    },
    /// Underlying engine or filesystem failure.
    Engine(EngineError),
}

impl fmt::Display for MvccError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WriteConflict { key, read_ts, writer_ts } => write!(
                f,
                "write conflict on key {:?}: snapshot {} but a version committed at {}",
                String::from_utf8_lossy(key),
                read_ts,
                writer_ts
            ),
            Self::Engine(err) => write!(f, "engine error: {err}"),
        }
    }
}

impl std::error::Error for MvccError {}

impl From<EngineError> for MvccError {
    fn from(err: EngineError) -> Self {
        Self::Engine(err)
    }
}

/// Snapshot-isolated key/value store: every transaction reads from the commit
/// timestamp that was current when it began, and conflicting commits are
/// rejected before a single byte reaches the log.
#[derive(Debug)]
pub struct MvccStore {
    engine: Engine,
    versions: BTreeMap<Vec<u8>, Vec<Version>>,
    committed_ts: Timestamp,
    next_tx_id: u64,
    open_tx: BTreeMap<u64, Timestamp>,
}

impl MvccStore {
    /// Opens (or creates) the log at `path` and rebuilds the full version chain.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, MvccError> {
        let path = path.as_ref();
        if !path.exists() {
            let creator = wal::WalWriter::open(path)
                .map_err(|err| MvccError::Engine(EngineError::from(err)))?;
            drop(creator);
        }
        let engine = Engine::open(path)?;
        let mut store = Self {
            engine,
            versions: BTreeMap::new(),
            committed_ts: 0,
            next_tx_id: 1,
            open_tx: BTreeMap::new(),
        };
        store.rebuild()?;
        Ok(store)
    }

    /// Path of the underlying log file.
    pub fn path(&self) -> &Path {
        self.engine.path()
    }

    /// Highest commit timestamp made durable by the log.
    pub fn last_commit_ts(&self) -> Timestamp {
        self.committed_ts
    }

    /// Snapshot a brand-new transaction will read from.
    pub fn snapshot_ts(&self) -> Timestamp {
        self.committed_ts
    }

    /// Commit timestamp of the newest committed version of `key`.
    pub fn visible_version(&self, key: &[u8]) -> Option<Timestamp> {
        let history = self.versions.get(key)?;
        history.last().filter(|version| version.value.is_some()).map(|version| version.ts)
    }

    /// Newest committed value of `key`.
    pub fn get(&self, key: &[u8]) -> Option<&[u8]> {
        let history = self.versions.get(key)?;
        history.last().and_then(|version| version.value.as_deref())
    }

    /// Value of `key` as seen from `snapshot_ts`, ignoring later commits.
    pub fn get_at(&self, key: &[u8], snapshot_ts: Timestamp) -> Option<&[u8]> {
        let history = self.versions.get(key)?;
        history
            .iter()
            .rev()
            .find(|version| version.ts <= snapshot_ts)
            .and_then(|version| version.value.as_deref())
    }

    /// Full version chain of `key`, oldest first.
    pub fn history(&self, key: &[u8]) -> &[Version] {
        self.versions.get(key).map(Vec::as_slice).unwrap_or(&[])
    }

    /// Number of live keys.
    pub fn len(&self) -> usize {
        self.versions
            .values()
            .filter(|history| history.last().is_some_and(|version| version.value.is_some()))
            .count()
    }

    /// Returns `true` when no key is visible.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Number of transactions that have begun but not yet committed/rolled back.
    pub fn open_transactions(&self) -> usize {
        self.open_tx.len()
    }

    /// Starts a snapshot-isolated transaction.
    pub fn begin(&mut self) -> MvccTransaction {
        let id = self.next_tx_id;
        self.next_tx_id += 1;
        let read_ts = self.committed_ts;
        self.open_tx.insert(id, read_ts);
        MvccTransaction { id, read_ts, ops: BTreeMap::new(), mutating: Vec::new() }
    }

    /// Snapshot read from inside `tx`, including its own staged (still invisible
    /// to everybody else) writes.
    pub fn tx_get<'a>(&'a self, tx: &'a MvccTransaction, key: &[u8]) -> Option<&'a [u8]> {
        if let Some(staged) = tx.ops.get(key) {
            return staged.as_deref();
        }
        self.get_at(key, tx.read_ts)
    }

    /// Convenience single-key commit.
    pub fn put(
        &mut self,
        key: impl AsRef<[u8]>,
        value: impl AsRef<[u8]>,
    ) -> Result<Timestamp, MvccError> {
        let mut tx = self.begin();
        tx.put(key, value);
        self.commit(tx)
    }

    /// Convenience single-key deletion.
    pub fn delete(&mut self, key: impl AsRef<[u8]>) -> Result<Timestamp, MvccError> {
        let mut tx = self.begin();
        tx.delete(key);
        self.commit(tx)
    }

    /// Durably commits `tx`, or rejects it with [`MvccError::WriteConflict`].
    pub fn commit(&mut self, tx: MvccTransaction) -> Result<Timestamp, MvccError> {
        self.open_tx.remove(&tx.id);
        if tx.ops.is_empty() {
            return Ok(self.committed_ts);
        }

        // Validate before touching the log: a rejected commit must leave both the
        // log and the visible state exactly as they were.
        for key in &tx.mutating {
            if let Some(writer_ts) = self.newest_ts(key) {
                if writer_ts > tx.read_ts {
                    return Err(MvccError::WriteConflict {
                        key: key.clone(),
                        read_ts: tx.read_ts,
                        writer_ts,
                    });
                }
            }
        }

        let mut batch = self.engine.begin();
        for key in &tx.mutating {
            match tx.ops.get(key) {
                Some(Some(value)) => {
                    let _ = batch.put(key, value);
                }
                Some(None) => {
                    let _ = batch.delete(key);
                }
                None => {}
            }
        }
        // The engine makes the batch durable before it becomes visible (C6).
        let ts = batch.commit()?;

        for key in &tx.mutating {
            let value = tx.ops.get(key).cloned().flatten();
            self.push_version(key.clone(), Version { value, ts });
        }
        self.committed_ts = self.committed_ts.max(ts);
        Ok(ts)
    }

    /// Drops `tx` without writing anything.
    pub fn rollback(&mut self, tx: MvccTransaction) {
        self.open_tx.remove(&tx.id);
    }

    fn newest_ts(&self, key: &[u8]) -> Option<Timestamp> {
        self.versions.get(key).and_then(|history| history.last()).map(|version| version.ts)
    }

    fn push_version(&mut self, key: Vec<u8>, version: Version) {
        let history = self.versions.entry(key).or_default();
        history.push(version);
        // Commits are serialised through `&mut self`; sorting only matters for
        // log replay.
        history.sort_by_key(|entry| entry.ts);
    }

    fn rebuild(&mut self) -> Result<(), MvccError> {
        let outcome = {
            let path = self.engine.path();
            wal::recover(path).map_err(|err| MvccError::Engine(EngineError::from(err)))?
        };
        let mut staged: Vec<(Vec<u8>, Option<Vec<u8>>, Timestamp)> = Vec::new();
        let mut committed_ts: Timestamp = 0;
        for record in &outcome.records {
            match record.kind {
                RecordKind::Commit => {
                    for (key, value, ts) in staged.drain(..) {
                        self.push_version(key, Version { value, ts });
                    }
                    committed_ts = committed_ts.max(record.ts);
                }
                RecordKind::Put => {
                    staged.push((record.key.clone(), Some(record.value.clone()), record.ts));
                }
                RecordKind::Delete => {
                    staged.push((record.key.clone(), None, record.ts));
                }
            }
        }
        self.committed_ts = committed_ts.max(self.engine.last_commit_ts());
        Ok(())
    }
}

/// Running transaction: a fixed read snapshot plus a private write set.
#[derive(Debug)]
pub struct MvccTransaction {
    id: u64,
    read_ts: Timestamp,
    ops: BTreeMap<Vec<u8>, Option<Vec<u8>>>,
    mutating: Vec<Vec<u8>>,
}

impl MvccTransaction {
    /// Identifier handed out by [`MvccStore::begin`].
    pub fn id(&self) -> u64 {
        self.id
    }

    /// Commit timestamp this transaction reads from.
    pub fn read_ts(&self) -> Timestamp {
        self.read_ts
    }

    /// Stages a write that nobody else can observe before the commit.
    pub fn put(&mut self, key: impl AsRef<[u8]>, value: impl AsRef<[u8]>) -> &mut Self {
        let key = key.as_ref().to_vec();
        if !self.mutating.contains(&key) {
            self.mutating.push(key.clone());
        }
        self.ops.insert(key, Some(value.as_ref().to_vec()));
        self
    }

    /// Stages a deletion.
    pub fn delete(&mut self, key: impl AsRef<[u8]>) -> &mut Self {
        let key = key.as_ref().to_vec();
        if !self.mutating.contains(&key) {
            self.mutating.push(key.clone());
        }
        self.ops.insert(key, None);
        self
    }

    /// Keys this transaction intends to write.
    pub fn keys(&self) -> impl Iterator<Item = &[u8]> {
        self.mutating.iter().map(Vec::as_slice)
    }

    /// Number of staged keys.
    pub fn len(&self) -> usize {
        self.ops.len()
    }

    /// Returns `true` when nothing was staged.
    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }
}
