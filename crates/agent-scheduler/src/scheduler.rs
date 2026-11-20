//! Readiness computation and batch starting.
//!
//! The two query functions are **pure** (contract C11): they never change a
//! status, never bump a sequence number, and can be called as often as needed.
//! Starting a task is always an explicit call to [`crate::transitions::start`]
//! (directly or through [`start_batch`]).

use crate::graph::TaskGraph;
use crate::task::{compare_schedule, Task, TaskId, TaskStatus};
use crate::transitions::{start, TransitionError};

/// A task is ready when it is `pending` and *every* dependency is `completed`.
///
/// A missing dependency (unvalidated imported graph) or a dependency that ended
/// in `failed` / `cancelled` blocks the task forever: readiness is never
/// optimistic.
fn is_ready(graph: &TaskGraph, task: &Task) -> bool {
    if task.status != TaskStatus::Pending {
        return false;
    }
    task.deps.iter().all(|dep| {
        graph
            .get(*dep)
            .map(|dependency| dependency.status == TaskStatus::Completed)
            .unwrap_or(false)
    })
}

/// Number of tasks currently running.
pub fn running_count(graph: &TaskGraph) -> usize {
    graph
        .tasks()
        .filter(|task| task.status == TaskStatus::Running)
        .count()
}

/// Every ready task, ordered by priority descending then insertion (`seq`)
/// ascending.
pub fn ready_set(graph: &TaskGraph) -> Vec<TaskId> {
    let mut ready: Vec<&Task> = graph
        .tasks()
        .filter(|task| is_ready(graph, task))
        .collect();
    ready.sort_by(|a, b| compare_schedule(a, b));
    ready.iter().map(|task| task.id).collect()
}

/// The tasks that should be started next, bounded by the free execution slots
/// (`max_concurrency` minus the tasks already running).
///
/// Pure: the caller decides, then starts them explicitly.
pub fn next_ready(graph: &TaskGraph, max_concurrency: usize) -> Vec<TaskId> {
    let free_slots = max_concurrency.saturating_sub(running_count(graph));
    let mut candidates = ready_set(graph);
    candidates.truncate(free_slots);
    candidates
}

/// Start every task selected by [`next_ready`] and return the started
/// identifiers, in scheduling order.
///
/// Every selected task is `pending` by construction, so no transition can fail;
/// the `Result` keeps the caller honest and would leave the graph untouched
/// before the first illegal start.
pub fn start_batch(
    graph: &mut TaskGraph,
    max_concurrency: usize,
) -> Result<Vec<TaskId>, TransitionError> {
    let batch = next_ready(graph, max_concurrency);
    for id in &batch {
        start(graph, *id)?;
    }
    Ok(batch)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transitions::{complete, fail, pause};

    // Contract C8 — readiness needs *all* dependencies completed.
    #[test]
    fn readiness_requires_all_dependencies_completed() {
        let mut graph = TaskGraph::new();
        graph.add_task(Task::new(1, 0)).unwrap();
        graph.add_task(Task::new(2, 0)).unwrap();
        graph.add_task(Task::with_deps(3, 0, &[1, 2])).unwrap();

        assert_eq!(ready_set(&graph), vec![1, 2]);
        assert!(!ready_set(&graph).contains(&3));

        start(&mut graph, 1).unwrap();
        // A running dependency is not a completed one.
        assert!(!ready_set(&graph).contains(&3));
        complete(&mut graph, 1).unwrap();
        // Only one of the two dependencies is done: still blocked.
        assert!(!ready_set(&graph).contains(&3));

        start(&mut graph, 2).unwrap();
        complete(&mut graph, 2).unwrap();
        assert_eq!(ready_set(&graph), vec![3]);

        // `failed` and `cancelled` dependencies block their dependents.
        let mut blocked = TaskGraph::new();
        blocked.add_task(Task::new(1, 0)).unwrap();
        blocked.add_task(Task::new(2, 0)).unwrap();
        blocked.add_task(Task::with_deps(3, 0, &[1])).unwrap();
        blocked.add_task(Task::with_deps(4, 0, &[2])).unwrap();
        start(&mut blocked, 1).unwrap();
        fail(&mut blocked, 1).unwrap();
        crate::transitions::cancel(&mut blocked, 2, false).unwrap();
        assert!(ready_set(&blocked).is_empty());

        // A paused task is not ready, and a missing dependency blocks as well.
        let mut paused = TaskGraph::new();
        paused.add_task(Task::new(1, 0)).unwrap();
        pause(&mut paused, 1).unwrap();
        assert!(ready_set(&paused).is_empty());

        let imported = TaskGraph::from_tasks(&[Task::with_deps(1, 0, &[99])]);
        assert!(ready_set(&imported).is_empty());
    }

    // Contract C9 — the concurrency budget is never exceeded.
    #[test]
    fn next_ready_respects_max_concurrency() {
        let mut graph = TaskGraph::new();
        for id in 1..=4 {
            graph.add_task(Task::new(id, 0)).unwrap();
        }

        assert_eq!(next_ready(&graph, 0), Vec::<TaskId>::new());
        assert_eq!(next_ready(&graph, 2), vec![1, 2]);
        assert_eq!(next_ready(&graph, 10), vec![1, 2, 3, 4]);

        assert_eq!(start_batch(&mut graph, 2).unwrap(), vec![1, 2]);
        assert_eq!(running_count(&graph), 2);
        // No free slot left: nothing can be started.
        assert!(next_ready(&graph, 2).is_empty());
        assert!(start_batch(&mut graph, 2).unwrap().is_empty());
        assert_eq!(running_count(&graph), 2);
        // A wider budget only yields the free slots.
        assert_eq!(next_ready(&graph, 3), vec![3]);
        assert_eq!(start_batch(&mut graph, 3).unwrap(), vec![3]);
        assert_eq!(running_count(&graph), 3);

        // Completing a task frees a slot -- but the budget is still the law.
        complete(&mut graph, 1).unwrap();
        assert_eq!(running_count(&graph), 2);
        // Tasks 2 and 3 were started under a budget of 3; with a budget of 2
        // the graph remains saturated, so nothing new may be started.
        assert_eq!(next_ready(&graph, 2), Vec::<TaskId>::new());
        assert!(start_batch(&mut graph, 2).unwrap().is_empty());
        assert_eq!(running_count(&graph), 2);
        // Under the budget of 3 that allowed task 3 to start, exactly one slot
        // is now free.
        assert_eq!(next_ready(&graph, 3), vec![4]);
        assert_eq!(running_count(&graph), 2);

        // Terminal tasks never come back into the batch.
        complete(&mut graph, 2).unwrap();
        complete(&mut graph, 3).unwrap();
        start(&mut graph, 4).unwrap();
        complete(&mut graph, 4).unwrap();
        assert!(next_ready(&graph, 8).is_empty());
    }

    // Contract C10 — deterministic order, priority descending, insertion ascending.
    #[test]
    fn next_ready_order_is_deterministic_and_priority_descending() {
        let mut graph = TaskGraph::new();
        graph.add_task(Task::new(1, 1)).unwrap();
        graph.add_task(Task::new(2, 9)).unwrap();
        graph.add_task(Task::new(3, 5)).unwrap();
        graph.add_task(Task::new(4, 9)).unwrap();

        let order = next_ready(&graph, 10);
        // 2 < 4 (same priority 9, seq 1 then 3), then 3 (priority 5), then 1.
        assert_eq!(order, vec![2, 4, 3, 1]);
        for _ in 0..8 {
            assert_eq!(next_ready(&graph, 10), order);
        }

        // A different insertion order yields a different tie-break, never a
        // non-deterministic one.
        let mut mirrored = TaskGraph::new();
        mirrored.add_task(Task::new(4, 9)).unwrap();
        mirrored.add_task(Task::new(2, 9)).unwrap();
        assert_eq!(next_ready(&mirrored, 10), vec![4, 2]);
        assert_eq!(next_ready(&mirrored, 10), next_ready(&mirrored, 10));

        // The batch honours the same order.
        let started = start_batch(&mut graph, 2).unwrap();
        assert_eq!(started, vec![2, 4]);
    }

    // Contract C11 — `next_ready` is pure.
    #[test]
    fn next_ready_is_pure() {
        let mut graph = TaskGraph::new();
        graph.add_task(Task::new(1, 3)).unwrap();
        graph.add_task(Task::with_deps(2, 1, &[1])).unwrap();
        let before = graph.clone();

        let preview = next_ready(&graph, 4);
        assert_eq!(preview, vec![1]);
        // Not a single byte changed: no status, no seq bump.
        assert_eq!(graph, before);
        assert_eq!(graph.get(1).unwrap().status, TaskStatus::Pending);
        assert_eq!(graph.get(1).unwrap().seq, 0);

        // Repeated previews are stable and still change nothing.
        assert_eq!(next_ready(&graph, 4), preview);
        assert_eq!(ready_set(&graph), vec![1]);
        assert_eq!(graph, before);

        // Only an explicit start mutates the graph.
        start(&mut graph, 1).unwrap();
        assert_ne!(graph, before);
        assert_eq!(graph.get(1).unwrap().status, TaskStatus::Running);
        assert!(next_ready(&graph, 4).is_empty());
    }
}