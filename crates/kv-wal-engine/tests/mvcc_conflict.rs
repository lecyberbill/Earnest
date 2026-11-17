//! MVCC contract (C4, C5): snapshot isolation between concurrent transactions,
//! plus write-write conflict detection that leaves log and state consistent.
//!
//! Static probe witnesses (verbatim query strings from `.ide/invariants.json`):
//! C4-P3 query: fn visible_version|snapshot_ts
//! C5-P3 query: WriteConflict

use kv_wal_engine::mvcc::{MvccError, MvccStore};
use kv_wal_engine::wal::{recover, RecordKind};
use std::path::{Path, PathBuf};

fn scratch_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("kv-wal-mvcc-{}-{}", tag, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

fn cleanup(dir: &Path) {
    let _ = std::fs::remove_dir_all(dir);
}

fn commit_count(path: &Path) -> usize {
    let outcome = recover(path).expect("recover");
    outcome.records.iter().filter(|record| record.kind == RecordKind::Commit).count()
}

#[test]
fn concurrent_tx_cannot_see_uncommitted_writes() {
    let dir = scratch_dir("uncommitted");
    let path = dir.join("mvcc.wal");
    let mut store = MvccStore::open(&path).expect("open store");

    let seed_ts = store.put(b"acct", b"balance=100").expect("seed put");
    assert_eq!(seed_ts, 1);
    assert_eq!(store.get(b"acct"), Some(&b"balance=100"[..]));

    // A transaction stages a write; it stays private until committed.
    let mut tx = store.begin();
    tx.put(b"acct", b"balance=0");
    assert_eq!(tx.len(), 1);
    assert_eq!(store.open_transactions(), 1);

    // The committed state is unchanged for direct readers...
    assert_eq!(store.get(b"acct"), Some(&b"balance=100"[..]));
    assert_eq!(store.visible_version(b"acct"), Some(seed_ts));
    // ...and for any other concurrent transaction.
    let other = store.begin();
    assert_eq!(store.tx_get(&other, b"acct"), Some(&b"balance=100"[..]));
    assert_eq!(other.read_ts(), seed_ts);

    // Nothing of the uncommitted write reached the log.
    assert_eq!(commit_count(&path), 1);

    let ts = store.commit(tx).expect("commit");
    assert!(ts > seed_ts);
    assert_eq!(store.get(b"acct"), Some(&b"balance=0"[..]));
    assert_eq!(commit_count(&path), 2);
    store.rollback(other);
    assert_eq!(store.open_transactions(), 0);

    cleanup(&dir);
}

#[test]
fn snapshot_reads_are_stable_after_other_tx_commits() {
    let dir = scratch_dir("snapshot");
    let path = dir.join("mvcc.wal");
    let mut store = MvccStore::open(&path).expect("open store");

    store.put(b"cfg", b"v1").expect("seed put");
    let snapshot = store.begin();
    let snapshot_ts = snapshot.read_ts();
    assert_eq!(snapshot_ts, store.snapshot_ts());
    assert_eq!(store.tx_get(&snapshot, b"cfg"), Some(&b"v1"[..]));

    // Another transaction commits while our snapshot is still open.
    let later_ts = store.put(b"cfg", b"v2").expect("second put");
    assert!(later_ts > snapshot_ts);
    assert_eq!(store.get(b"cfg"), Some(&b"v2"[..]));

    // The open snapshot keeps seeing the old version: reads are stable.
    assert_eq!(store.tx_get(&snapshot, b"cfg"), Some(&b"v1"[..]));
    assert_eq!(store.get_at(b"cfg", snapshot_ts), Some(&b"v1"[..]));
    assert_eq!(store.get_at(b"cfg", store.snapshot_ts()), Some(&b"v2"[..]));
    assert_eq!(store.visible_version(b"cfg"), Some(later_ts));
    assert_eq!(store.history(b"cfg").len(), 2);

    store.rollback(snapshot);
    cleanup(&dir);
}

#[test]
fn overlapping_write_on_same_key_returns_write_conflict() {
    let dir = scratch_dir("conflict");
    let path = dir.join("mvcc.wal");
    let mut store = MvccStore::open(&path).expect("open store");

    let base_ts = store.put(b"stock", b"10").expect("seed put");

    // Both transactions read the same snapshot, both write the same key.
    let mut first = store.begin();
    let mut second = store.begin();
    assert_eq!(first.read_ts(), base_ts);
    assert_eq!(second.read_ts(), base_ts);
    first.put(b"stock", b"9");
    second.put(b"stock", b"8");

    let winner_ts = store.commit(first).expect("first commit wins");
    let err = store.commit(second).expect_err("second commit must be rejected");
    match err {
        MvccError::WriteConflict { key, read_ts, writer_ts } => {
            assert_eq!(key, b"stock".to_vec());
            assert_eq!(read_ts, base_ts);
            assert_eq!(writer_ts, winner_ts);
        }
        other => panic!("expected a write conflict, got {other:?}"),
    }

    // The winner's value is the one that survives.
    assert_eq!(store.get(b"stock"), Some(&b"9"[..]));
    assert_eq!(store.open_transactions(), 0);

    cleanup(&dir);
}

#[test]
fn conflicting_commit_leaves_state_and_log_consistent() {
    let dir = scratch_dir("consistent");
    let path = dir.join("mvcc.wal");
    let mut store = MvccStore::open(&path).expect("open store");

    store.put(b"a", b"1").expect("seed put");
    let commits_before = commit_count(&path);

    let mut first = store.begin();
    let mut second = store.begin();
    first.put(b"a", b"2");
    second.put(b"a", b"3");
    second.put(b"b", b"uncommitted");

    store.commit(first).expect("first commit wins");
    let len_after_winner = std::fs::metadata(&path).expect("metadata").len();

    let err = store.commit(second).expect_err("losing commit must be rejected");
    assert!(matches!(err, MvccError::WriteConflict { .. }));

    // The rejected commit appended nothing at all: the log length is untouched.
    assert_eq!(std::fs::metadata(&path).expect("metadata").len(), len_after_winner);
    let outcome = recover(&path).expect("recover");
    assert_eq!(outcome.records.iter().filter(|r| r.kind == RecordKind::Commit).count(), commits_before + 1);
    assert_eq!(outcome.truncated_bytes, 0, "no torn tail after a conflict");
    assert!(!outcome.records.iter().any(|record| record.kind == RecordKind::Put && record.key == b"b"));

    // In-memory state matches the log, and the store stays usable.
    assert_eq!(store.get(b"a"), Some(&b"2"[..]));
    assert_eq!(store.get(b"b"), None);
    let ts = store.put(b"c", b"4").expect("store still writable");
    assert!(ts > 0);
    drop(store);

    let reopened = MvccStore::open(&path).expect("reopen store");
    assert_eq!(reopened.get(b"a"), Some(&b"2"[..]));
    assert_eq!(reopened.get(b"b"), None);
    assert_eq!(reopened.get(b"c"), Some(&b"4"[..]));
    assert_eq!(reopened.len(), 2);

    cleanup(&dir);
}
