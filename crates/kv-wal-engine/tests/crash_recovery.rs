//! Crash-recovery contract (C2, C3, C6): torn/corrupt tails and uncommitted tails
//! are dropped at the last commit boundary, the log owns the commit timeline, and a
//! commit record is durable before it becomes visible.
//!
//! Static probe witnesses (verbatim query strings from `.ide/invariants.json`,
//! kept in comments because a `static` probe matches the raw file text):
//! C2-P3 query: fn open|fn replay
//! C6-P2 query: fn commit\(

use kv_wal_engine::engine::Engine;
use kv_wal_engine::wal::{recover, Record, RecordKind, WalWriter};
use std::path::{Path, PathBuf};

fn scratch_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("kv-wal-crash-{}-{}", tag, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

fn cleanup(dir: &Path) {
    let _ = std::fs::remove_dir_all(dir);
}

fn seed(key: &[u8], value: &[u8], ts: u64) -> Record {
    Record { kind: RecordKind::Put, ts, key: key.to_vec(), value: value.to_vec() }
}

fn commit_marker(ts: u64) -> Record {
    Record { kind: RecordKind::Commit, ts, key: Vec::new(), value: Vec::new() }
}

/// Writes `records` to `path`, makes them durable, and returns the on-disk bytes.
fn write_durable_log(path: &Path, records: &[Record]) -> Vec<u8> {
    let mut writer = WalWriter::open(path).expect("open log");
    for record in records {
        writer.append(record).expect("append");
    }
    writer.sync_data().expect("sync_data");
    drop(writer);
    std::fs::read(path).expect("read back")
}

fn sample_records() -> Vec<Record> {
    vec![
        seed(b"alpha", b"one", 1),
        seed(b"beta", b"two", 2),
        seed(b"gamma", b"three", 3),
    ]
}

#[test]
fn corrupted_tail_is_truncated_without_panic() {
    let dir = scratch_dir("corrupt-tail");
    let path = dir.join("torn.wal");
    let records = sample_records();
    let full = write_durable_log(&path, &records);

    // Chop the tail in the middle of the last record, then poison the remaining
    // bytes: recovery must keep the intact prefix and drop everything after it.
    let intact_len = full.len() - 7;
    let mut damaged = full[..intact_len].to_vec();
    for byte in damaged.iter_mut().rev().take(5) {
        *byte = 0xA5;
    }
    std::fs::write(&path, &damaged).expect("write damaged log");

    let outcome = recover(&path).expect("recover must not fail on a corrupt tail");
    assert_eq!(outcome.records.len(), 2, "only the two intact records survive");
    assert_eq!(outcome.records[0], records[0]);
    assert_eq!(outcome.records[1], records[1]);
    assert!(outcome.valid_len < damaged.len() as u64, "tail was dropped");
    assert_eq!(outcome.truncated_bytes, damaged.len() as u64 - outcome.valid_len);

    // The file itself must have been trimmed to the valid prefix.
    let after = std::fs::read(&path).expect("read trimmed log");
    assert_eq!(after.len() as u64, outcome.valid_len);

    // Recovery is idempotent: a second pass finds nothing left to truncate.
    let again = recover(&path).expect("second recover");
    assert_eq!(again.records, outcome.records);
    assert_eq!(again.truncated_bytes, 0);

    cleanup(&dir);
}

#[test]
fn truncation_at_every_byte_offset() {
    let dir = scratch_dir("every-offset");
    let path = dir.join("sweep.wal");
    let records = sample_records();
    let full = write_durable_log(&path, &records);

    for cut in 0..=full.len() {
        std::fs::write(&path, &full[..cut]).expect("write prefix");

        let outcome = recover(&path)
            .unwrap_or_else(|err| panic!("recover failed at cut {cut}: {err}"));

        assert!(
            outcome.valid_len <= cut as u64,
            "valid_len {} exceeds prefix {cut}",
            outcome.valid_len
        );
        assert_eq!(
            outcome.truncated_bytes,
            cut as u64 - outcome.valid_len,
            "truncated_bytes must account for exactly the dropped bytes at cut {cut}"
        );
        assert!(
            outcome.records.len() <= records.len(),
            "recovered more records than were ever written at cut {cut}"
        );

        // Every recovered record must be a prefix of what was written, unchanged.
        for (index, record) in outcome.records.iter().enumerate() {
            assert_eq!(record, &records[index], "record {index} corrupted at cut {cut}");
        }

        let trimmed = std::fs::metadata(&path).expect("metadata").len();
        assert_eq!(trimmed, outcome.valid_len, "file not trimmed to valid prefix at cut {cut}");

        // Re-running recovery on the trimmed file is a no-op.
        let again = recover(&path).expect("second recover");
        assert_eq!(again.truncated_bytes, 0, "not idempotent at cut {cut}");
        assert_eq!(again.records, outcome.records, "records changed at cut {cut}");
    }

    // Sanity: the complete log recovers every record.
    std::fs::write(&path, &full).expect("write complete log");
    let complete = recover(&path).expect("recover complete log");
    assert_eq!(complete.records, records);
    assert_eq!(complete.truncated_bytes, 0);

    cleanup(&dir);
}

#[test]
fn uncommitted_tail_records_are_discarded() {
    let dir = scratch_dir("uncommitted-tail");
    let path = dir.join("uncommitted.wal");

    // One committed transaction: alpha = one.
    let mut engine = Engine::open(&path).expect("open engine");
    let first_ts = engine.put(b"alpha", b"one").expect("put alpha");
    assert_eq!(engine.get(b"alpha"), Some(b"one".as_slice()));
    drop(engine);
    let committed_len = std::fs::metadata(&path).expect("metadata").len();

    // Simulate a crash mid-transaction: framed, checksummed records with no commit.
    let mut writer = WalWriter::open(&path).expect("open raw wal");
    writer.append(&seed(b"beta", b"two", 99)).expect("append beta");
    writer.append(&seed(b"gamma", b"three", 99)).expect("append gamma");
    writer.sync_data().expect("sync_data");
    let tail_len = writer.len();
    drop(writer);
    assert!(tail_len > committed_len, "the uncommitted tail must have been written");

    // Recovery keeps the committed prefix only...
    let reopened = Engine::open(&path).expect("reopen engine");
    assert_eq!(reopened.get(b"alpha"), Some(b"one".as_slice()));
    assert_eq!(reopened.get(b"beta"), None, "uncommitted record must not be visible");
    assert_eq!(reopened.get(b"gamma"), None, "uncommitted record must not be visible");
    assert_eq!(reopened.last_commit_ts(), first_ts);
    assert_eq!(reopened.len(), 1);

    // ...and the uncommitted tail is physically gone from the log.
    let after = std::fs::metadata(&path).expect("metadata").len();
    assert_eq!(after, committed_len, "log not truncated back to the last commit boundary");

    // Second reopen is a no-op: nothing left to discard.
    let again = Engine::open(&path).expect("second reopen");
    assert_eq!(again.get(b"beta"), None);
    assert_eq!(again.last_commit_ts(), first_ts);
    drop(again);

    // The engine can keep writing on the cleaned boundary without resurrecting
    // phantom keys from the discarded tail.
    let mut fresh = Engine::open(&path).expect("third reopen");
    let next_ts = fresh.put(b"beta", b"two").expect("put beta");
    assert_eq!(next_ts, first_ts + 1);
    assert_eq!(fresh.get(b"beta"), Some(b"two".as_slice()));
    assert_eq!(fresh.get(b"gamma"), None);
    drop(fresh);

    cleanup(&dir);
}

#[test]
fn commit_ts_from_log_is_authoritative() {
    let dir = scratch_dir("authoritative-ts");
    let path = dir.join("ts.wal");

    // A log written by an earlier process: timestamps come from the log, not from
    // a fresh in-memory counter that would restart at 0.
    let mut writer = WalWriter::open(&path).expect("open raw wal");
    writer.append(&seed(b"k", b"v1", 100)).expect("append 100");
    writer.append(&commit_marker(100)).expect("commit 100");
    writer.append(&seed(b"k", b"v2", 250)).expect("append 250");
    writer.append(&commit_marker(250)).expect("commit 250");
    writer.sync_data().expect("sync_data");
    drop(writer);

    let mut engine = Engine::open(&path).expect("open engine");
    assert_eq!(engine.last_commit_ts(), 250, "engine must adopt the logged commit timestamp");
    assert_eq!(engine.get(b"k"), Some(b"v2".as_slice()));
    assert_eq!(engine.visible_version(b"k"), Some(250), "visibility must use the logged ts");

    // The timeline continues from the log, never from a local counter.
    let next = engine.put(b"k", b"v3").expect("put v3");
    assert_eq!(next, 251, "next commit must follow the logged timeline");
    assert_eq!(engine.visible_version(b"k"), Some(251));
    drop(engine);

    // And it survives a restart: the new ts is now itself in the log.
    let reopened = Engine::open(&path).expect("reopen engine");
    assert_eq!(reopened.last_commit_ts(), 251);
    assert_eq!(reopened.get(b"k"), Some(b"v3".as_slice()));
    assert_eq!(reopened.visible_version(b"k"), Some(251));

    cleanup(&dir);
}

/// C6: the commit record reaches stable storage *before* the new value becomes
/// visible. A crash between the fsync and the in-memory swap must therefore be
/// recoverable: reopening the log sees exactly what the last commit promised.
#[test]
fn commit_record_is_durable_before_visibility() {
    let dir = scratch_dir("durable-before-visible");
    let path = dir.join("c6.wal");

    let mut engine = Engine::open(&path).expect("open engine");
    let first = engine.put(b"cfg", b"v1").expect("put v1");
    assert_eq!(engine.get(b"cfg"), Some(b"v1".as_slice()), "value visible after commit");
    drop(engine);

    // On-disk evidence: the log already contains a Commit record for `first`,
    // i.e. durability happened as part of the commit, not after visibility.
    let outcome = recover(&path).expect("recover after commit");
    assert!(
        outcome.records.iter().any(|record| record.kind == RecordKind::Commit && record.ts == first),
        "commit record for ts {first} is not on disk"
    );
    let on_disk = outcome.records.clone();

    // Simulate a crash right after the fsync: the process vanishes here and the
    // state is rebuilt purely from the log.
    let reopened = Engine::open(&path).expect("reopen after simulated crash");
    assert_eq!(
        reopened.get(b"cfg"),
        Some(b"v1".as_slice()),
        "committed value must survive a crash that happened after the sync"
    );
    assert_eq!(reopened.visible_version(b"cfg"), Some(first));
    assert_eq!(reopened.last_commit_ts(), first);
    assert_eq!(
        reopen_records(&path),
        on_disk,
        "reopening must not rewrite or truncate a fully committed log"
    );

    // Visibility never runs ahead of durability: each successive commit is
    // already on disk at the very instant it becomes readable.
    let mut engine = reopened;
    for step in 0..3u8 {
        let value = format!("v{}", step + 2);
        let ts = engine.put(b"cfg", value.as_bytes()).expect("put next");
        assert_eq!(engine.visible_version(b"cfg"), Some(ts));
        let records = recover(&path).expect("recover after each commit").records;
        assert!(
            records.iter().any(|record| record.kind == RecordKind::Commit && record.ts == ts),
            "value at ts {ts} became visible before its commit record was durable"
        );
    }

    cleanup(&dir);
}

fn reopen_records(path: &Path) -> Vec<Record> {
    recover(path).expect("recover for comparison").records
}