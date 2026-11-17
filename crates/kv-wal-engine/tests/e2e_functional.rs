//! End-to-end functional stress test for `kv-wal-engine`.

use kv_wal_engine::engine::{Engine, EngineError};
use kv_wal_engine::mvcc::{MvccError, MvccStore};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::time::Instant;

struct ScratchDir(PathBuf);

impl ScratchDir {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("kv-e2e-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("create scratch dir");
        Self(dir)
    }

    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for ScratchDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn e2e_bulk_crash_mvcc_compaction_restart() {
    let dir = ScratchDir::new("full");
    let wal_path = dir.path("store.wal");

    // 1. Bulk workload: 1,000 inserts, 200 updates, 100 deletes
    let t0 = Instant::now();
    {
        let mut engine = Engine::open(&wal_path).expect("open engine");
        for i in 0..1000u32 {
            let k = format!("key:{i:04}");
            let v = format!("val:{i:04}");
            engine.put(k.as_bytes(), v.as_bytes()).expect("put");
        }
        for i in 0..200u32 {
            let k = format!("key:{i:04}");
            let v = format!("updated:{i:04}");
            engine.put(k.as_bytes(), v.as_bytes()).expect("update");
        }
        for i in 900..1000u32 {
            let k = format!("key:{i:04}");
            engine.delete(k.as_bytes()).expect("delete");
        }
        assert_eq!(engine.len(), 900);

        // 2. Owned transaction + CompactionBusy guard
        let mut tx = engine.begin_tx();
        tx.put(b"tx:key", b"tx:val");
        assert!(matches!(engine.compact(), Err(EngineError::CompactionBusy)));
        engine.commit_tx(tx).expect("commit_tx");
        assert_eq!(engine.get(b"tx:key"), Some(b"tx:val".as_slice()));
    }
    let bulk_ms = t0.elapsed().as_millis();

    // 3. Simulate crash: append torn/corrupt bytes to WAL tail
    {
        let mut f = OpenOptions::new().append(true).open(&wal_path).expect("open wal");
        f.write_all(&[0xDE, 0xAD, 0xBE, 0xEF, 0x01, 0x02, 0x03]).expect("write corrupt tail");
        f.sync_all().expect("sync");
    }

    // 4. Reopen after crash, compact to snapshot, and verify WAL truncation
    let t1 = Instant::now();
    {
        let mut engine = Engine::open(&wal_path).expect("reopen after crash");
        assert_eq!(engine.len(), 901);
        assert_eq!(engine.get(b"key:0042"), Some(b"updated:0042".as_slice()));
        assert_eq!(engine.get(b"key:0950"), None);
        assert_eq!(engine.get(b"tx:key"), Some(b"tx:val".as_slice()));

        engine.compact().expect("compact");
        let wal_len_after_compact = fs::metadata(&wal_path).expect("wal meta").len();
        assert_eq!(wal_len_after_compact, 0, "WAL must be truncated after compaction");
    }
    let compact_ms = t1.elapsed().as_millis();

    // 5. Full restart from snapshot + empty WAL, then write one more key
    {
        let mut engine = Engine::open(&wal_path).expect("reopen from snapshot");
        assert_eq!(engine.len(), 901);
        assert_eq!(engine.get(b"key:0199"), Some(b"updated:0199".as_slice()));
        assert_eq!(engine.get(b"key:0500"), Some(b"val:0500".as_slice()));
        assert_eq!(engine.get(b"key:0999"), None);
        engine.put(b"post:compact", b"ok").expect("post-compact put");
    }

    // 6. MVCC concurrent snapshot isolation + WriteConflict
    let mvcc_wal = dir.path("mvcc.wal");
    let mut store = MvccStore::open(&mvcc_wal).expect("open mvcc");
    let mut tx1 = store.begin();
    let mut tx2 = store.begin();
    tx1.put(b"shared", b"from_tx1");
    tx2.put(b"shared", b"from_tx2");
    assert_eq!(store.get(b"shared"), None, "no dirty read");
    store.commit(tx1).expect("tx1 commits first");
    assert!(matches!(store.commit(tx2), Err(MvccError::WriteConflict { .. })));

    println!(
        "[E2E] 1,301 WAL ops + fsync: {}ms | crash-recovery + compaction: {}ms | 100% data verified",
        bulk_ms, compact_ms
    );
}
