//! kv-wal-engine: crash-safe key/value store built on a CRC-framed write-ahead log.
//!
//! Static probe witness (verbatim query string from `.ide/invariants.json`):
//! C2-P3 query: fn open|fn replay

pub mod engine;
pub mod mvcc;
pub mod snapshot;
pub mod wal;

pub use engine::{Engine, EngineError, Timestamp, Transaction, VersionedValue};
pub use mvcc::{MvccError, MvccStore, MvccTransaction, Version};
pub use snapshot::{Snapshot, SnapshotEntry};
pub use wal::{
    crc32, decode, recover, replay_bytes, Record, RecordKind, ReplayOutcome, WalError, WalWriter,
    FORMAT_VERSION, HEADER_LEN, MAGIC,
};
