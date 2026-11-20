//! Aggregate progress view over a whole graph.
//!
//! The snapshot is a pure function of the graph: counting is done in a single
//! ordered traversal, so two identical graphs always produce an identical
//! summary.

use crate::graph::TaskGraph;
use crate::task::TaskStatus;

/// Counter snapshot of every status present in a graph.
///
/// Invariant (checked by contract C15):
/// `total == pending + running + paused + completed + failed + cancelled`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ProgressSummary {
    /// Number of tasks in the graph.
    pub total: usize,
    /// Tasks waiting for their dependencies.
    pub pending: usize,
    /// Tasks currently executing.
    pub running: usize,
    /// Tasks suspended by the operator.
    pub paused: usize,
    /// Tasks finished successfully.
    pub completed: usize,
    /// Tasks finished with an error.
    pub failed: usize,
    /// Tasks given up.
    pub cancelled: usize,
}

impl ProgressSummary {
    /// Count every status of `graph` in one ordered pass.
    pub fn from_graph(graph: &TaskGraph) -> Self {
        let mut summary = Self::default();
        for task in graph.tasks() {
            summary.total += 1;
            match task.status {
                TaskStatus::Pending => summary.pending += 1,
                TaskStatus::Running => summary.running += 1,
                TaskStatus::Paused => summary.paused += 1,
                TaskStatus::Completed => summary.completed += 1,
                TaskStatus::Failed => summary.failed += 1,
                TaskStatus::Cancelled => summary.cancelled += 1,
            }
        }
        summary
    }

    /// Tasks that reached a terminal status.
    pub fn terminal(&self) -> usize {
        self.completed + self.failed + self.cancelled
    }

    /// Tasks that are still going to make progress (`pending`, `running`,
    /// `paused`).
    pub fn active(&self) -> usize {
        self.pending + self.running + self.paused
    }

    /// No task is left to schedule. An empty graph counts as finished (there is
    /// nothing left to do).
    pub fn is_finished(&self) -> bool {
        self.active() == 0
    }

    /// Share of successfully completed tasks, in `0.0..=1.0`.
    ///
    /// Returns `0.0` for an empty graph.
    pub fn completion_ratio(&self) -> f64 {
        if self.total == 0 {
            0.0
        } else {
            self.completed as f64 / self.total as f64
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::task::Task;

    // Contract C15 — the summary is internally consistent and deterministic.
    #[test]
    fn progress_summary_is_internally_consistent() {
        let mut graph = TaskGraph::new();
        for id in 1..=6 {
            graph.add_task(Task::new(id, 0)).unwrap();
        }
        graph.get_mut(1).unwrap().status = TaskStatus::Running;
        graph.get_mut(2).unwrap().status = TaskStatus::Paused;
        graph.get_mut(3).unwrap().status = TaskStatus::Completed;
        graph.get_mut(4).unwrap().status = TaskStatus::Failed;
        graph.get_mut(5).unwrap().status = TaskStatus::Cancelled;
        // 6 stays `pending`.

        let summary = ProgressSummary::from_graph(&graph);
        assert_eq!(summary.total, graph.len());
        assert_eq!(summary.total, 6);
        assert_eq!(summary.pending, 1);
        assert_eq!(summary.running, 1);
        assert_eq!(summary.paused, 1);
        assert_eq!(summary.completed, 1);
        assert_eq!(summary.failed, 1);
        assert_eq!(summary.cancelled, 1);

        // The core invariant: every task is counted exactly once.
        assert_eq!(
            summary.pending
                + summary.running
                + summary.paused
                + summary.completed
                + summary.failed
                + summary.cancelled,
            summary.total
        );
        assert_eq!(summary.active(), 3);
        assert_eq!(summary.terminal(), 3);
        assert_eq!(summary.active() + summary.terminal(), summary.total);
        assert!(!summary.is_finished());
        assert_eq!(summary.completion_ratio(), 1.0 / 6.0);

        // Deterministic: repeated snapshots are identical.
        assert_eq!(ProgressSummary::from_graph(&graph), summary);
        assert_eq!(ProgressSummary::from_graph(&graph), ProgressSummary::from_graph(&graph));

        // Empty graph.
        let empty = ProgressSummary::from_graph(&TaskGraph::new());
        assert_eq!(empty.total, 0);
        assert_eq!(empty.active(), 0);
        assert_eq!(empty.terminal(), 0);
        assert!(empty.is_finished());
        assert_eq!(empty.completion_ratio(), 0.0);

        // Everything terminal -> finished, ratio 1.0 only when all completed.
        for id in graph.ids() {
            graph.get_mut(id).unwrap().status = TaskStatus::Completed;
        }
        let done = ProgressSummary::from_graph(&graph);
        assert_eq!(done.completed, 6);
        assert_eq!(done.terminal(), 6);
        assert_eq!(done.active(), 0);
        assert!(done.is_finished());
        assert_eq!(done.completion_ratio(), 1.0);

        // A graph that ended in failure is finished but not successful.
        graph.get_mut(1).unwrap().status = TaskStatus::Failed;
        let partial = ProgressSummary::from_graph(&graph);
        assert!(partial.is_finished());
        assert_eq!(partial.completed, 5);
        assert_eq!(partial.failed, 1);
        assert_eq!(partial.completion_ratio(), 5.0 / 6.0);
    }
}