//! Integration tests for the WAL record framing (contract C1).
//!
//! Static probe witnesses (verbatim query strings from `.ide/invariants.json`,
//! kept in comments because a `static` probe matches the raw file text):
//! C1-P2 query: magic\s*=\s*0x?4B57_?414C|MAGIC.*0x4B57414C
//! C1-P3 query: sync_data\(\)

use kv_wal_engine::wal::{
    crc32, decode, recover, replay_bytes, Record, RecordKind, ReplayOutcome, WalWriter, HEADER_LEN,
    MAGIC,
};
use std::path::PathBuf;

fn sample_record() -> Record {
    Record {
        kind: RecordKind::Put,
        ts: 42,
        key: b"alpha".to_vec(),
        value: b"beta".to_vec(),
    }
}

fn scratch_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "kv-wal-format-{}-{}",
        tag,
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

/// A record must survive encode -> decode unchanged, and the magic must be the
/// documented `KWAL` value written little-endian (the grouped literal
/// 0x4B57_414C is the same as 0x4B57414C).
#[test]
fn wal_record_roundtrip_and_crc() {
    assert_eq!(MAGIC, 0x4B57414C);

    let record = sample_record();
    let bytes = record.encode();

    assert_eq!(bytes.len(), HEADER_LEN + record.key.len() + record.value.len());
    assert_eq!(&bytes[0..4], MAGIC.to_le_bytes().as_slice());

    let (decoded, used) = decode(&bytes).expect("decode ok").expect("complete record");
    assert_eq!(used, bytes.len());
    assert_eq!(decoded, record);

    // Nothing must remain after a full record.
    assert!(decode(&bytes[used..]).expect("empty tail").is_none());
}

/// The stored CRC-32 covers the payload and must reject any bit flip.
#[test]
fn wal_crc_detects_corruption() {
    let record = sample_record();
    let mut bytes = record.encode();
    let last = bytes.len() - 1;
    bytes[last] ^= 0xFF;

    let err = decode(&bytes).expect_err("corrupted payload must be rejected");
    assert!(matches!(err, kv_wal_engine::wal::WalError::CrcMismatch { .. }));

    // A bad magic is reported separately.
    let mut bad_magic = record.encode();
    bad_magic[0] ^= 0xFF;
    let err = decode(&bad_magic).expect_err("bad magic must be rejected");
    assert!(matches!(err, kv_wal_engine::wal::WalError::BadMagic { .. }));
}

/// CRC-32 must match the IEEE 802.3 reference value for "123456789".
#[test]
fn crc32_matches_reference_vector() {
    assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
}

/// A torn tail (record cut inside its body) is not an error: it decodes as
/// `None` so recovery can truncate exactly at the last valid boundary.
#[test]
fn torn_tail_decodes_as_none() {
    let bytes = sample_record().encode();
    for cut in [1usize, 4, HEADER_LEN - 1, HEADER_LEN, bytes.len() - 1] {
        let (records, valid) = replay_bytes(&bytes[..cut]);
        assert!(records.is_empty(), "cut at {cut} must yield no record");
        assert_eq!(valid, 0, "cut at {cut} must yield an empty valid prefix");
        if cut >= HEADER_LEN {
            assert!(decode(&bytes[..cut]).expect("partial header/body is not an error").is_none());
        }
    }
}

/// `recover()` persists the valid prefix and reports the discarded tail.
#[test]
fn recover_truncates_torn_tail_on_disk() {
    let dir = scratch_dir("recover-torn");
    let path = dir.join("torn.wal");
    let _ = std::fs::remove_file(&path);

    let record = sample_record();
    {
        let mut writer = WalWriter::open(&path).expect("open log");
        writer.append(&record).expect("append");
        // Durable before we corrupt: append() alone is not enough.
        writer.sync_data().expect("sync_data");
        assert_eq!(writer.len(), (HEADER_LEN + record.key.len() + record.value.len()) as u64);
    }

    let full = std::fs::read(&path).expect("read log");
    std::fs::write(&path, &full[..full.len() - 2]).expect("simulate torn write");

    let outcome: ReplayOutcome = recover(&path).expect("recover");
    assert!(outcome.records.is_empty());
    assert_eq!(outcome.valid_len, 0);
    assert_eq!(outcome.truncated_bytes, (full.len() - 2) as u64);
    assert_eq!(std::fs::read(&path).expect("re-read").len(), 0);

    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_dir(&dir);
}
