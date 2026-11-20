//! `agent-scheduler` — deterministic dependency-aware task scheduler.
//!
//! Design constraints sealed by the project invariants (see `.ide/atlas.md`):
//! * `std` only, no external crate (contract C16).
//! * Every collection traversal is ordered (`BTreeMap` / `BTreeSet`), a
//!   `HashMap` is never iterated.
//! * Every rejected operation leaves the graph untouched (contract C2).
//! * [`scheduler::next_ready`] is pure and never mutates the graph (contract C11).

pub mod graph;
pub mod scheduler;
pub mod summary;
pub mod task;
pub mod transitions;

pub use graph::{GraphError, TaskGraph, ValidationIssue};
pub use scheduler::{next_ready, ready_set, running_count, start_batch};
pub use summary::ProgressSummary;
pub use task::{compare_schedule, Task, TaskId, TaskStatus};
pub use transitions::{cancel, complete, fail, pause, resume, start, TransitionError};
