//! Compaction & restart contract (C7, C8, C9): a compaction publishes an atomic
//! snapshot next to the log, truncates the log it supersedes, survives a cold
//! restart, and is refused while a detached transaction is still open.
//!
//! Scratch directories are built from `std::env::temp_dir` plus a self-cleaning
//! `TempDir` guard, so the crate keeps zero external dependencies.
//!
//! Static probe witnesses (verbatim query strings from `.ide/invariants.json`):
//! C7-P3 query: rename|sync_all|tmp
//! C10-P3 query: tempfile|TempDir

use kv_wal_engine::{snapshot, Engine, EngineError, Timestamp};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// Minimal self-cleaning scratch directory (no external crates allowed).
struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(tag: &str) -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let unique = COUNTER.fetch_add(1, Ordering::SeqCst);
        let mut path = std::env::temp_dir();
        path.push(format!(
            "kv-wal-engine-{tag}-{}-{unique}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).expect("create scratch dir");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// Size in bytes of the log file, `0` when it does not exist yet.
fn log_len(path: &Path) -> u64 {
    std::fs::metadata(path).map(|meta| meta.len()).unwrap_or(0)
}

/// Writes `count` distinct keys, each with its own commit.
fn seed_keys(engine: &mut Engine, count: u8) {
    for i in 0..count {
        let key = format!("key-{i}");
        let value = format!("value-{i}");
        engine
            .put(key.as_bytes(), value.as_bytes())
            .expect("seed put");
    }
}

#[test]
fn snapshot_is_written_via_tmp_and_rename() {
    let dir = TempDir::new("tmp-rename");
    let path = dir.path().join("store.wal");

    let mut engine = Engine::open(&path).expect("open engine");
    seed_keys(&mut engine, 3);

    let published = engine.compact().expect("compact");
    assert!(published.is_file(), "the snapshot exists after compaction");
    assert_eq!(published, snapshot::path_for(&path));

    // The intermediate file is gone: publication ends with a rename.
    assert!(
        !snapshot::tmp_path_for(&path).exists(),
        "no temporary file survives a successful publication"
    );

    // The published image is a real snapshot of the live state.
    let image = snapshot::load(&path)
        .expect("load snapshot")
        .expect("snapshot present");
    assert_eq!(image.entries.len(), 3);
    assert_eq!(image.last_ts, engine.last_commit_ts());
    let key_zero = image
        .entries
        .iter()
        .find(|entry| entry.key == b"key-0")
        .expect("key-0 captured in the snapshot");
    assert_eq!(key_zero.value, b"value-0");
}

#[test]
fn corrupted_snapshot_is_rejected() {
    let dir = TempDir::new("corrupt-snapshot");
    let path = dir.path().join("store.wal");

    let mut engine = Engine::open(&path).expect("open engine");
    seed_keys(&mut engine, 2);
    let published = engine.compact().expect("compact");
    let good = std::fs::read(&published).expect("read snapshot");

    // 1. A flipped payload byte must fail the checksum.
    let mut flipped = good.clone();
    let middle = flipped.len() / 2;
    flipped[middle] ^= 0xFF;
    std::fs::write(&published, &flipped).expect("write corrupted snapshot");
    assert!(
        snapshot::load(&path).is_err(),
        "checksum mismatch must be reported"
    );

    // 2. A truncated image must be refused, not half-parsed.
    let truncated = good[..good.len() / 2].to_vec();
    std::fs::write(&published, &truncated).expect("write truncated snapshot");
    assert!(
        snapshot::load(&path).is_err(),
        "truncated snapshot must be rejected"
    );

    // 3. Trailing garbage is not silently ignored.
    let mut trailing = good.clone();
    trailing.extend_from_slice(b"stray");
    std::fs::write(&published, &trailing).expect("write padded snapshot");
    assert!(
        snapshot::load(&path).is_err(),
        "trailing bytes must be rejected"
    );

    // The untouched image still round-trips once restored.
    std::fs::write(&published, &good).expect("restore snapshot");
    assert!(snapshot::load(&path).expect("load").is_some());
}

#[test]
fn compaction_truncates_wal_and_preserves_all_data() {
    let dir = TempDir::new("truncate-wal");
    let path = dir.path().join("store.wal");

    let mut engine = Engine::open(&path).expect("open engine");
    seed_keys(&mut engine, 8);
    // Overwrite one key: compaction must keep the live version only.
    engine.put(b"key-0", b"final").expect("overwrite");

    let before = log_len(&path);
    assert!(before > 0, "the log holds records before compaction");

    engine.compact().expect("compact");

    assert_eq!(log_len(&path), 0, "compaction truncates the log it supersedes");
    assert_eq!(engine.len(), 8, "compaction drops nothing that is live");
    assert_eq!(
        engine.get(b"key-0"),
        Some(b"final".as_slice()),
        "the latest version wins"
    );
    assert_eq!(engine.last_commit_ts(), 9);

    // Nothing is lost across a cold restart driven by the snapshot alone.
    drop(engine);
    let reopened = Engine::open(&path).expect("reopen after compaction");
    assert_eq!(reopened.len(), 8);
    assert_eq!(reopened.get(b"key-0"), Some(b"final".as_slice()));
    assert_eq!(reopened.get(b"key-3"), Some(b"value-3".as_slice()));
    assert_eq!(reopened.last_commit_ts(), 9);
}

#[test]
fn replay_over_fresh_snapshot_is_idempotent() {
    let dir = TempDir::new("idempotent-replay");
    let path = dir.path().join("store.wal");

    let mut engine = Engine::open(&path).expect("open engine");
    engine.put(b"alpha", b"one").expect("put alpha");
    engine.put(b"beta", b"two").expect("put beta");
    engine.delete(b"beta").expect("delete beta");
    engine.compact().expect("compact");
    drop(engine);

    let mut baseline: Option<(usize, Timestamp)> = None;
    for round in 0..3 {
        let reopened = Engine::open(&path).expect("reopen");
        let observed = (reopened.len(), reopened.last_commit_ts());
        if let Some(expected) = baseline {
            assert_eq!(observed, expected, "round {round}: replay must not drift");
        }
        assert_eq!(reopened.get(b"alpha"), Some(b"one".as_slice()));
        assert_eq!(reopened.get(b"beta"), None, "a deleted key stays deleted");
        assert_eq!(
            log_len(&path),
            0,
            "replay never rewrites the truncated log"
        );
        baseline = Some(observed);
    }
}

#[test]
fn full_restart_from_disk_after_compaction() {
    let dir = TempDir::new("restart");
    let path = dir.path().join("store.wal");

    let mut engine = Engine::open(&path).expect("open engine");
    seed_keys(&mut engine, 5);
    engine.compact().expect("compact");

    // Writes made after a compaction land in a fresh log layered on the snapshot.
    engine.put(b"after-1", b"a1").expect("put after compaction");
    engine.put(b"after-2", b"a2").expect("put after compaction");
    assert!(log_len(&path) > 0, "new writes re-populate the log");
    drop(engine);

    let reopened = Engine::open(&path).expect("cold restart");
    assert_eq!(reopened.len(), 7, "snapshot keys plus post-compaction keys");
    for i in 0..5u8 {
        let key = format!("key-{i}");
        let value = format!("value-{i}");
        assert_eq!(
            reopened.get(key.as_bytes()),
            Some(value.as_bytes()),
            "key {key} survives the restart"
        );
    }
    assert_eq!(reopened.get(b"after-2"), Some(b"a2".as_slice()));
    assert_eq!(
        reopened.last_commit_ts(),
        7,
        "the commit timeline continues across the restart"
    );
}

#[test]
fn compact_with_active_tx_returns_compaction_busy() {
    let dir = TempDir::new("compaction-busy");
    let path = dir.path().join("store.wal");

    let mut engine = Engine::open(&path).expect("open engine");
    engine.put(b"alpha", b"one").expect("put alpha");

    let mut tx = engine.begin_tx();
    tx.put(b"beta", b"two");
    tx.delete(b"alpha");
    assert_eq!(tx.len(), 2);
    assert_eq!(
        engine.open_transactions(),
        1,
        "the detached transaction is tracked by the engine"
    );

    let refusal = engine.compact();
    assert!(
        matches!(&refusal, Err(EngineError::CompactionBusy)),
        "compaction must be refused while a transaction is open, got {refusal:?}"
    );

    // The refusal changed nothing: same data, transaction still open, log intact.
    assert_eq!(engine.get(b"alpha"), Some(b"one".as_slice()));
    assert_eq!(engine.open_transactions(), 1);
    assert!(log_len(&path) > 0, "a refused compaction keeps the log");

    engine.rollback_tx(tx);
    assert_eq!(engine.open_transactions(), 0);
    let published = engine
        .compact()
        .expect("compaction runs once the transaction is gone");
    assert!(published.is_file());
    assert_eq!(log_len(&path), 0);
}
