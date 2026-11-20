//! Core task model: identifier, priority, insertion sequence and lifecycle status.
//!
//! Determinism starts here: the owning graph assigns an insertion sequence
//! number (`seq`) to every task, used as the tie-breaker when two tasks share
//! the same priority.

use std::cmp::Ordering;
use std::fmt;

/// Unique identifier of a task inside a [`TaskGraph`](crate::graph::TaskGraph).
pub type TaskId = u64;

/// Lifecycle status of a task.
///
/// The six states are exhaustive. Any transition that is not listed in
/// [`crate::transitions`] is rejected and leaves the graph untouched.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TaskStatus {
    /// Created, waiting for its dependencies to complete.
    Pending,
    /// Currently executing.
    Running,
    /// Suspended by the operator; only `resume`/`cancel` may act on it.
    Paused,
    /// Terminal: finished successfully.
    Completed,
    /// Terminal: finished with an error. Does not propagate.
    Failed,
    /// Terminal: given up (propagates transitively through `cancel`).
    Cancelled,
}

impl TaskStatus {
    /// Every status, in declaration order (used by the exhaustive matrix tests).
    pub const ALL: [TaskStatus; 6] = [
        TaskStatus::Pending,
        TaskStatus::Running,
        TaskStatus::Paused,
        TaskStatus::Completed,
        TaskStatus::Failed,
        TaskStatus::Cancelled,
    ];

    /// A terminal status can never be left again.
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            TaskStatus::Completed | TaskStatus::Failed | TaskStatus::Cancelled
        )
    }

    /// Stable textual name (stable output for logs and reports).
    pub fn name(self) -> &'static str {
        match self {
            TaskStatus::Pending => "pending",
            TaskStatus::Running => "running",
            TaskStatus::Paused => "paused",
            TaskStatus::Completed => "completed",
            TaskStatus::Failed => "failed",
            TaskStatus::Cancelled => "cancelled",
        }
    }
}

impl fmt::Display for TaskStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// A unit of work together with its dependencies.
///
/// `seq` is owned by the graph: the value carried by a freshly built `Task` is
/// irrelevant, it is overwritten at insertion time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Task {
    /// Identifier, unique inside the graph.
    pub id: TaskId,
    /// Scheduling priority; greater value = scheduled first.
    pub priority: u8,
    /// Insertion sequence assigned by the graph (ties broken by ascending seq).
    pub seq: u64,
    /// Current lifecycle status.
    pub status: TaskStatus,
    /// Dependencies: this task may only become ready once *all* of them are
    /// `Completed`. Order is preserved, duplicates are removed at build time.
    pub deps: Vec<TaskId>,
}

impl Task {
    /// Build a task without dependency.
    pub fn new(id: TaskId, priority: u8) -> Self {
        Self {
            id,
            priority,
            seq: 0,
            status: TaskStatus::Pending,
            deps: Vec::new(),
        }
    }

    /// Build a task depending on `deps` (duplicates are removed, order kept).
    pub fn with_deps(id: TaskId, priority: u8, deps: &[TaskId]) -> Self {
        let mut unique: Vec<TaskId> = Vec::with_capacity(deps.len());
        for dep in deps {
            if !unique.contains(dep) {
                unique.push(*dep);
            }
        }
        Self {
            id,
            priority,
            seq: 0,
            status: TaskStatus::Pending,
            deps: unique,
        }
    }

    /// A terminal task is never scheduled again.
    pub fn is_terminal(&self) -> bool {
        self.status.is_terminal()
    }

    /// Whether `dep` is a direct dependency of this task.
    pub fn depends_on(&self, dep: TaskId) -> bool {
        self.deps.contains(&dep)
    }
}

/// Total order used by the scheduler: priority **descending**, then `seq`
/// **ascending** (insertion order). Two distinct tasks never compare `Equal`
/// because `seq` is unique.
pub fn compare_schedule(a: &Task, b: &Task) -> Ordering {
    b.priority.cmp(&a.priority).then(a.seq.cmp(&b.seq))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_helpers_are_consistent() {
        assert!(TaskStatus::Completed.is_terminal());
        assert!(TaskStatus::Failed.is_terminal());
        assert!(TaskStatus::Cancelled.is_terminal());
        assert!(!TaskStatus::Pending.is_terminal());
        assert!(!TaskStatus::Running.is_terminal());
        assert!(!TaskStatus::Paused.is_terminal());

        let task = Task::with_deps(7, 3, &[1, 2, 1]);
        assert_eq!(task.deps, vec![1, 2]);
        assert!(task.depends_on(2));
        assert!(!task.depends_on(9));
        assert_eq!(task.status, TaskStatus::Pending);

        let mut high = Task::new(1, 9);
        high.seq = 5;
        let mut low = Task::new(2, 1);
        low.seq = 0;
        assert_eq!(compare_schedule(&high, &low), Ordering::Less);
    }
}
