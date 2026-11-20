//! Legal lifecycle transitions.
//!
//! Invariants enforced here:
//! * every operation validates the pair `(current status, target status)`
//!   *before* touching anything, so a rejected call leaves the graph unchanged;
//! * the matrix is exhaustive and closed: a terminal status is never left again;
//! * `fail` and `pause` never propagate (contract C13);
//! * `cancel(.., propagate = true)` cancels the transitive closure of the
//!   dependents but never modifies a terminal task: those are only traversed
//!   (contract C12).

use std::collections::BTreeSet;
use std::error::Error;
use std::fmt;

use crate::graph::TaskGraph;
use crate::task::{TaskId, TaskStatus};

/// Error returned by every lifecycle transition.
///
/// Getting one of these guarantees the graph was not modified.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransitionError {
    /// No task with this identifier exists in the graph.
    UnknownTask(TaskId),
    /// The pair `(from, to)` is not part of the transition matrix, or the entry
    /// point used by the caller does not accept `from`.
    IllegalTransition {
        task: TaskId,
        from: TaskStatus,
        to: TaskStatus,
    },
}

impl fmt::Display for TransitionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TransitionError::UnknownTask(id) => write!(f, "unknown task {id}"),
            TransitionError::IllegalTransition { task, from, to } => {
                write!(f, "illegal transition for task {task}: {from} -> {to}")
            }
        }
    }
}

impl Error for TransitionError {}

/// The complete transition matrix.
///
/// Legal edges: `pending -> running | paused | cancelled`,
/// `running -> completed | failed | paused | cancelled`,
/// `paused -> running | cancelled`. Everything else is rejected, and the three
/// terminal statuses (`completed`, `failed`, `cancelled`) have no outgoing edge.
pub fn is_legal(from: TaskStatus, to: TaskStatus) -> bool {
    use TaskStatus::*;
    matches!(
        (from, to),
        (Pending, Running)
            | (Pending, Paused)
            | (Pending, Cancelled)
            | (Running, Completed)
            | (Running, Failed)
            | (Running, Paused)
            | (Running, Cancelled)
            | (Paused, Running)
            | (Paused, Cancelled)
    )
}

fn status_of(graph: &TaskGraph, id: TaskId) -> Result<TaskStatus, TransitionError> {
    graph
        .get(id)
        .map(|task| task.status)
        .ok_or(TransitionError::UnknownTask(id))
}

/// Validate, then mutate: the graph is only touched once the transition is
/// known to be legal.
///
/// `allowed_from` restricts the entry points: both `start` and `resume` land on
/// `Running`, but `start` only accepts `Pending` while `resume` only accepts
/// `Paused`.
fn transact(
    graph: &mut TaskGraph,
    id: TaskId,
    allowed_from: &[TaskStatus],
    to: TaskStatus,
) -> Result<(), TransitionError> {
    let from = status_of(graph, id)?;
    if !allowed_from.contains(&from) || !is_legal(from, to) {
        return Err(TransitionError::IllegalTransition { task: id, from, to });
    }
    match graph.get_mut(id) {
        Some(task) => {
            task.status = to;
            Ok(())
        }
        // Unreachable: `status_of` just proved the task exists.
        None => Err(TransitionError::UnknownTask(id)),
    }
}

/// `pending -> running`. A paused task must go through [`resume`].
pub fn start(graph: &mut TaskGraph, id: TaskId) -> Result<(), TransitionError> {
    transact(graph, id, &[TaskStatus::Pending], TaskStatus::Running)
}

/// `running -> completed`.
pub fn complete(graph: &mut TaskGraph, id: TaskId) -> Result<(), TransitionError> {
    transact(graph, id, &[TaskStatus::Running], TaskStatus::Completed)
}

/// `running -> failed`. Does not propagate to the dependents.
pub fn fail(graph: &mut TaskGraph, id: TaskId) -> Result<(), TransitionError> {
    transact(graph, id, &[TaskStatus::Running], TaskStatus::Failed)
}

/// `pending | running -> paused`. Does not propagate to the dependents.
pub fn pause(graph: &mut TaskGraph, id: TaskId) -> Result<(), TransitionError> {
    transact(
        graph,
        id,
        &[TaskStatus::Pending, TaskStatus::Running],
        TaskStatus::Paused,
    )
}

/// `paused -> running`. A resumed task is running: it is not “ready” anymore.
pub fn resume(graph: &mut TaskGraph, id: TaskId) -> Result<(), TransitionError> {
    transact(graph, id, &[TaskStatus::Paused], TaskStatus::Running)
}

/// `pending | running | paused -> cancelled`.
///
/// When `propagate` is set, the transitive closure of the dependents is
/// cancelled as well. Terminal tasks (`completed`, `failed`, `cancelled`) are
/// never modified, they are only traversed so that a pending task sitting
/// *behind* a failed one is still reached.
///
/// Returns the identifiers actually cancelled, in ascending order. On error the
/// graph is left untouched.
pub fn cancel(
    graph: &mut TaskGraph,
    id: TaskId,
    propagate: bool,
) -> Result<Vec<TaskId>, TransitionError> {
    // 1. Validate the root first: nothing has been mutated yet.
    let from = status_of(graph, id)?;
    if !is_legal(from, TaskStatus::Cancelled) {
        return Err(TransitionError::IllegalTransition {
            task: id,
            from,
            to: TaskStatus::Cancelled,
        });
    }

    // 2. Collect the whole closure read-only, so the decision is atomic.
    let mut to_cancel: BTreeSet<TaskId> = BTreeSet::new();
    to_cancel.insert(id);
    if propagate {
        let mut visited: BTreeSet<TaskId> = BTreeSet::new();
        visited.insert(id);
        let mut stack: Vec<TaskId> = vec![id];
        while let Some(current) = stack.pop() {
            for dependent in graph.dependents(current) {
                if !visited.insert(dependent) {
                    continue;
                }
                let already_terminal = graph
                    .get(dependent)
                    .map(|task| task.status.is_terminal())
                    .unwrap_or(true);
                if !already_terminal {
                    to_cancel.insert(dependent);
                }
                // Traverse through terminal tasks as well.
                stack.push(dependent);
            }
        }
    }

    // 3. Apply, now that every decision has been taken.
    for task_id in &to_cancel {
        if let Some(task) = graph.get_mut(*task_id) {
            task.status = TaskStatus::Cancelled;
        }
    }

    Ok(to_cancel.into_iter().collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::task::Task;

    fn task_with(id: TaskId, status: TaskStatus) -> Task {
        let mut task = Task::new(id, 0);
        task.status = status;
        task
    }

    fn graph_with_statuses(items: &[(TaskId, TaskStatus)]) -> TaskGraph {
        let tasks: Vec<Task> = items
            .iter()
            .map(|(id, status)| task_with(*id, *status))
            .collect();
        TaskGraph::from_tasks(&tasks)
    }

    /// Dispatch on the *target* status, the way a caller would.
    fn apply_target(
        graph: &mut TaskGraph,
        id: TaskId,
        to: TaskStatus,
    ) -> Result<(), TransitionError> {
        let from = graph.get(id).unwrap().status;
        match to {
            // No operation brings a task back to `pending`.
            TaskStatus::Pending => Err(TransitionError::IllegalTransition { task: id, from, to }),
            TaskStatus::Running => match from {
                TaskStatus::Paused => resume(graph, id),
                _ => start(graph, id),
            },
            TaskStatus::Paused => pause(graph, id),
            TaskStatus::Completed => complete(graph, id),
            TaskStatus::Failed => fail(graph, id),
            TaskStatus::Cancelled => cancel(graph, id, false).map(|_| ()),
        }
    }

    // Contract C1 — the transition matrix is exhaustive and closed.
    #[test]
    fn transition_matrix_is_exhaustive() {
        let expected = |from: TaskStatus, to: TaskStatus| {
            use TaskStatus::*;
            matches!(
                (from, to),
                (Pending, Running)
                    | (Pending, Paused)
                    | (Pending, Cancelled)
                    | (Running, Completed)
                    | (Running, Failed)
                    | (Running, Paused)
                    | (Running, Cancelled)
                    | (Paused, Running)
                    | (Paused, Cancelled)
            )
        };

        // All 36 pairs are covered, in declaration order (deterministic).
        assert_eq!(TaskStatus::ALL.len(), 6);
        for from in TaskStatus::ALL {
            for to in TaskStatus::ALL {
                assert_eq!(is_legal(from, to), expected(from, to), "matrix {from} -> {to}");

                let mut graph = graph_with_statuses(&[(1, from)]);
                let before = graph.clone();
                let outcome = apply_target(&mut graph, 1, to);

                if expected(from, to) {
                    assert!(outcome.is_ok(), "{from} -> {to} must be accepted");
                    assert_eq!(graph.get(1).unwrap().status, to);
                } else {
                    assert_eq!(
                        outcome,
                        Err(TransitionError::IllegalTransition { task: 1, from, to }),
                        "{from} -> {to} must be rejected"
                    );
                    assert_eq!(graph, before, "a rejected {from} -> {to} must not mutate");
                }
            }
        }

        // Terminal statuses have no outgoing edge at all.
        for terminal in [
            TaskStatus::Completed,
            TaskStatus::Failed,
            TaskStatus::Cancelled,
        ] {
            assert!(TaskStatus::ALL
                .iter()
                .all(|to| !is_legal(terminal, *to)));
        }

        // Unknown identifiers are rejected, and nothing is created.
        let mut empty = TaskGraph::new();
        assert_eq!(start(&mut empty, 42), Err(TransitionError::UnknownTask(42)));
        assert_eq!(pause(&mut empty, 42), Err(TransitionError::UnknownTask(42)));
        assert_eq!(resume(&mut empty, 42), Err(TransitionError::UnknownTask(42)));
        assert_eq!(complete(&mut empty, 42), Err(TransitionError::UnknownTask(42)));
        assert_eq!(fail(&mut empty, 42), Err(TransitionError::UnknownTask(42)));
        assert_eq!(
            cancel(&mut empty, 42, true),
            Err(TransitionError::UnknownTask(42))
        );
        assert!(empty.is_empty());
    }

    // Contract C12 — cancellation propagates, but never touches terminal tasks.
    #[test]
    fn cancel_propagates_transitively_without_touching_terminal_tasks() {
        let mut graph = TaskGraph::new();
        graph.add_task(Task::new(1, 0)).unwrap();
        graph.add_task(Task::with_deps(2, 0, &[1])).unwrap();
        graph.add_task(Task::with_deps(3, 0, &[2])).unwrap();
        graph.add_task(Task::with_deps(4, 0, &[3])).unwrap();
        graph.add_task(Task::new(5, 0)).unwrap();
        graph.add_task(Task::with_deps(6, 0, &[1])).unwrap();

        // 3 already failed: terminal, so it must be traversed but not modified.
        graph.get_mut(3).unwrap().status = TaskStatus::Failed;

        let cancelled = cancel(&mut graph, 1, true).unwrap();
        // 4 sits behind the terminal 3 and is still reached.
        assert_eq!(cancelled, vec![1, 2, 4, 6]);
        assert_eq!(graph.get(1).unwrap().status, TaskStatus::Cancelled);
        assert_eq!(graph.get(2).unwrap().status, TaskStatus::Cancelled);
        assert_eq!(graph.get(3).unwrap().status, TaskStatus::Failed);
        assert_eq!(graph.get(4).unwrap().status, TaskStatus::Cancelled);
        assert_eq!(graph.get(5).unwrap().status, TaskStatus::Pending);
        assert_eq!(graph.get(6).unwrap().status, TaskStatus::Cancelled);

        // A completed dependent is left alone as well.
        let mut other = TaskGraph::new();
        other.add_task(Task::new(10, 0)).unwrap();
        other.add_task(Task::with_deps(11, 0, &[10])).unwrap();
        other.get_mut(11).unwrap().status = TaskStatus::Completed;
        assert_eq!(cancel(&mut other, 10, true).unwrap(), vec![10]);
        assert_eq!(other.get(11).unwrap().status, TaskStatus::Completed);

        // Without propagation only the root changes.
        let mut local = TaskGraph::new();
        local.add_task(Task::new(1, 0)).unwrap();
        local.add_task(Task::with_deps(2, 0, &[1])).unwrap();
        assert_eq!(cancel(&mut local, 1, false).unwrap(), vec![1]);
        assert_eq!(local.get(1).unwrap().status, TaskStatus::Cancelled);
        assert_eq!(local.get(2).unwrap().status, TaskStatus::Pending);

        // A terminal root cannot be cancelled, and the graph stays intact.
        let before = graph.clone();
        assert!(matches!(
            cancel(&mut graph, 3, true),
            Err(TransitionError::IllegalTransition { .. })
        ));
        assert_eq!(graph, before);
    }

    // Contract C13 — `fail` and `pause` are strictly local.
    #[test]
    fn fail_and_pause_do_not_propagate() {
        let mut graph = TaskGraph::new();
        graph.add_task(Task::new(1, 0)).unwrap();
        graph.add_task(Task::with_deps(2, 0, &[1])).unwrap();
        graph.add_task(Task::with_deps(3, 0, &[2])).unwrap();

        start(&mut graph, 1).unwrap();
        pause(&mut graph, 1).unwrap();
        assert_eq!(graph.get(1).unwrap().status, TaskStatus::Paused);
        assert_eq!(graph.get(2).unwrap().status, TaskStatus::Pending);
        assert_eq!(graph.get(3).unwrap().status, TaskStatus::Pending);

        resume(&mut graph, 1).unwrap();
        fail(&mut graph, 1).unwrap();
        assert_eq!(graph.get(1).unwrap().status, TaskStatus::Failed);
        assert_eq!(graph.get(2).unwrap().status, TaskStatus::Pending);
        assert_eq!(graph.get(3).unwrap().status, TaskStatus::Pending);

        // A failed task is frozen.
        let before = graph.clone();
        for outcome in [
            start(&mut graph, 1),
            resume(&mut graph, 1),
            pause(&mut graph, 1),
            complete(&mut graph, 1),
            fail(&mut graph, 1),
        ] {
            assert!(matches!(
                outcome,
                Err(TransitionError::IllegalTransition { .. })
            ));
        }
        assert_eq!(graph, before);
    }

    // Contract C14 — `resume` brings the task back to running, out of the ready set.
    #[test]
    fn resume_returns_task_to_running_and_excluded_from_ready() {
        use crate::scheduler::ready_set;

        let mut graph = TaskGraph::new();
        graph.add_task(Task::new(1, 5)).unwrap();
        graph.add_task(Task::with_deps(2, 9, &[1])).unwrap();

        assert_eq!(ready_set(&graph), vec![1]);

        pause(&mut graph, 1).unwrap();
        assert_eq!(graph.get(1).unwrap().status, TaskStatus::Paused);
        // A paused task is neither ready nor startable by `start`.
        assert!(ready_set(&graph).is_empty());
        assert!(matches!(
            start(&mut graph, 1),
            Err(TransitionError::IllegalTransition { .. })
        ));
        assert!(matches!(
            complete(&mut graph, 1),
            Err(TransitionError::IllegalTransition { .. })
        ));

        resume(&mut graph, 1).unwrap();
        assert_eq!(graph.get(1).unwrap().status, TaskStatus::Running);
        // Running: no longer ready, and the dependent is still blocked.
        assert!(ready_set(&graph).is_empty());
        assert!(!ready_set(&graph).contains(&2));

        complete(&mut graph, 1).unwrap();
        assert_eq!(ready_set(&graph), vec![2]);
    }
}