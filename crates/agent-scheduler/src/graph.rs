//! Dependency graph: the single source of truth for tasks and their edges.
//!
//! Invariants enforced here:
//! * insertion through [`TaskGraph::add_task`] / [`TaskGraph::add_tasks`] is
//!   *atomic*: a rejected operation leaves the graph byte-for-byte untouched;
//! * forward references are only allowed *inside* a batch (`add_tasks`), never
//!   for a single `add_task`;
//! * the graph is cycle-free by construction, and [`TaskGraph::validate`] is
//!   able to report *all* anomalies of an imported (unchecked) graph, in a
//!   deterministic order;
//! * every traversal is ordered (`BTreeMap` / `BTreeSet`), never a `HashMap`.

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt;

use crate::task::{Task, TaskId};

/// Error returned by every mutating graph operation.
///
/// Getting one of these guarantees the graph was not modified.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GraphError {
    /// A task with this identifier already exists.
    DuplicateId(TaskId),
    /// A task cannot depend on itself.
    SelfDependency(TaskId),
    /// A dependency is neither in the graph nor in the submitted batch.
    MissingDependency { task: TaskId, dep: TaskId },
    /// The operation would (or an imported graph does) contain a cycle.
    /// Members are sorted ascending so the payload is comparable.
    CycleDetected(Vec<TaskId>),
}

impl fmt::Display for GraphError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GraphError::DuplicateId(id) => write!(f, "duplicate task id {id}"),
            GraphError::SelfDependency(id) => write!(f, "task {id} depends on itself"),
            GraphError::MissingDependency { task, dep } => {
                write!(f, "task {task} depends on unknown task {dep}")
            }
            GraphError::CycleDetected(members) => write!(f, "dependency cycle between {members:?}"),
        }
    }
}

impl Error for GraphError {}

/// Anomaly reported by [`TaskGraph::validate`].
///
/// The variant declaration order defines the deterministic sort order of the
/// report: `SelfDependency` < `DuplicateDependency` < `MissingDependency` <
/// `Cycle`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum ValidationIssue {
    /// A task lists itself among its own dependencies.
    SelfDependency { task: TaskId },
    /// A dependency is listed more than once by the same task.
    DuplicateDependency { task: TaskId, dep: TaskId },
    /// A dependency does not exist in the graph.
    MissingDependency { task: TaskId, dep: TaskId },
    /// The dependency edges form a cycle (`members` sorted ascending).
    Cycle { members: Vec<TaskId> },
}

impl fmt::Display for ValidationIssue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ValidationIssue::SelfDependency { task } => {
                write!(f, "task {task} depends on itself")
            }
            ValidationIssue::DuplicateDependency { task, dep } => {
                write!(f, "task {task} lists dependency {dep} twice")
            }
            ValidationIssue::MissingDependency { task, dep } => {
                write!(f, "task {task} depends on unknown task {dep}")
            }
            ValidationIssue::Cycle { members } => {
                write!(f, "dependency cycle between {members:?}")
            }
        }
    }
}

/// Directed acyclic graph of [`Task`]s (edges point from dependency to
/// dependent).
///
/// `seq` is assigned by the graph at insertion time and is what makes the
/// scheduling order total and reproducible.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TaskGraph {
    tasks: BTreeMap<TaskId, Task>,
    next_seq: u64,
}

impl TaskGraph {
    /// Empty graph.
    pub fn new() -> Self {
        Self {
            tasks: BTreeMap::new(),
            next_seq: 0,
        }
    }

    /// Import tasks **without any validation** (persistence, fixtures).
    ///
    /// Insertion order defines `seq`. Such a graph may violate the invariants:
    /// call [`TaskGraph::validate`] before relying on it, or use
    /// [`TaskGraph::add_task`] / [`TaskGraph::add_tasks`] for checked input.
    pub fn from_tasks(tasks: &[Task]) -> Self {
        let mut graph = Self::new();
        for task in tasks {
            let mut stored = task.clone();
            stored.seq = graph.next_seq;
            graph.next_seq += 1;
            graph.tasks.insert(stored.id, stored);
        }
        graph
    }

    /// Number of tasks.
    pub fn len(&self) -> usize {
        self.tasks.len()
    }

    /// Whether the graph holds no task.
    pub fn is_empty(&self) -> bool {
        self.tasks.is_empty()
    }

    /// Whether `id` is present.
    pub fn contains(&self, id: TaskId) -> bool {
        self.tasks.contains_key(&id)
    }

    /// Immutable access to a task.
    pub fn get(&self, id: TaskId) -> Option<&Task> {
        self.tasks.get(&id)
    }

    /// Mutable access to a task (used by the transitions module).
    pub fn get_mut(&mut self, id: TaskId) -> Option<&mut Task> {
        self.tasks.get_mut(&id)
    }

    /// Every task, in ascending identifier order.
    pub fn tasks(&self) -> impl Iterator<Item = &Task> + '_ {
        self.tasks.values()
    }

    /// Every identifier, in ascending order.
    pub fn ids(&self) -> Vec<TaskId> {
        self.tasks.keys().copied().collect()
    }

    /// Direct dependents of `id` (tasks listing it as a dependency), sorted.
    pub fn dependents(&self, id: TaskId) -> Vec<TaskId> {
        self.tasks
            .iter()
            .filter(|(_, task)| task.depends_on(id))
            .map(|(task_id, _)| *task_id)
            .collect()
    }

    /// Insert one task, checking every invariant.
    ///
    /// Rejected (graph untouched) when the identifier is already used, when the
    /// task depends on itself, when a dependency does not exist yet, or when the
    /// resulting graph would contain a (transitive) cycle.
    pub fn add_task(&mut self, task: Task) -> Result<(), GraphError> {
        let id = task.id;
        self.check_single(&task)?;

        let mut candidate = self.tasks.clone();
        let mut stored = task;
        stored.seq = self.next_seq;
        candidate.insert(id, stored.clone());

        // The invariant is checked on the *whole* graph, so a transitive cycle
        // (e.g. 1 <- 2 <- 3 <- 1) is never accepted, even when it pre-existed.
        if let Some(members) = find_cycle(&candidate) {
            return Err(GraphError::CycleDetected(members));
        }

        self.tasks.insert(id, stored);
        self.next_seq += 1;
        Ok(())
    }

    /// Insert a batch of tasks atomically.
    ///
    /// Forward references are allowed **inside** the batch (a task may depend on
    /// a task declared later in the same slice). If any task is invalid, none of
    /// them is inserted.
    pub fn add_tasks(&mut self, tasks: &[Task]) -> Result<(), GraphError> {
        let mut staged: BTreeMap<TaskId, Task> = BTreeMap::new();
        for task in tasks {
            if self.tasks.contains_key(&task.id) || staged.contains_key(&task.id) {
                return Err(GraphError::DuplicateId(task.id));
            }
            if task.deps.contains(&task.id) {
                return Err(GraphError::SelfDependency(task.id));
            }
            staged.insert(task.id, task.clone());
        }

        for task in staged.values() {
            for dep in &task.deps {
                if !self.tasks.contains_key(dep) && !staged.contains_key(dep) {
                    return Err(GraphError::MissingDependency {
                        task: task.id,
                        dep: *dep,
                    });
                }
            }
        }

        let mut candidate = self.tasks.clone();
        for (id, task) in &staged {
            candidate.insert(*id, task.clone());
        }
        if let Some(members) = find_cycle(&candidate) {
            return Err(GraphError::CycleDetected(members));
        }

        // Commit phase: nothing can fail from here on.
        for task in tasks {
            let mut stored = task.clone();
            stored.seq = self.next_seq;
            self.next_seq += 1;
            self.tasks.insert(stored.id, stored);
        }
        Ok(())
    }

    /// Report **all** anomalies of the current content, deterministically
    /// sorted (see [`ValidationIssue`]).
    pub fn validate(&self) -> Vec<ValidationIssue> {
        let mut issues = Vec::new();
        for (id, task) in &self.tasks {
            let mut seen: BTreeSet<TaskId> = BTreeSet::new();
            for dep in &task.deps {
                if !seen.insert(*dep) {
                    issues.push(ValidationIssue::DuplicateDependency {
                        task: *id,
                        dep: *dep,
                    });
                    continue;
                }
                if dep == id {
                    issues.push(ValidationIssue::SelfDependency { task: *id });
                    continue;
                }
                if !self.tasks.contains_key(dep) {
                    issues.push(ValidationIssue::MissingDependency {
                        task: *id,
                        dep: *dep,
                    });
                }
            }
        }
        if let Some(members) = find_cycle(&self.tasks) {
            issues.push(ValidationIssue::Cycle { members });
        }
        issues.sort();
        issues
    }

    /// Topological order (dependencies before dependents), deterministic: Kahn's
    /// algorithm driven by a `BTreeSet` ready queue, so the smallest ready
    /// identifier always comes first.
    pub fn topo_order(&self) -> Result<Vec<TaskId>, GraphError> {
        let mut indegree: BTreeMap<TaskId, usize> = BTreeMap::new();
        let mut edges: BTreeMap<TaskId, BTreeSet<TaskId>> = BTreeMap::new();

        for (id, task) in &self.tasks {
            let mut unique: BTreeSet<TaskId> = BTreeSet::new();
            for dep in &task.deps {
                if !unique.insert(*dep) {
                    continue;
                }
                if !self.tasks.contains_key(dep) {
                    return Err(GraphError::MissingDependency {
                        task: *id,
                        dep: *dep,
                    });
                }
                edges.entry(*dep).or_default().insert(*id);
            }
            indegree.insert(*id, unique.len());
        }

        let mut ready: BTreeSet<TaskId> = indegree
            .iter()
            .filter(|(_, degree)| **degree == 0)
            .map(|(id, _)| *id)
            .collect();
        let mut order: Vec<TaskId> = Vec::with_capacity(self.tasks.len());

        while let Some(next) = ready.iter().next().copied() {
            ready.remove(&next);
            order.push(next);
            if let Some(dependents) = edges.get(&next) {
                for dependent in dependents {
                    if let Some(degree) = indegree.get_mut(dependent) {
                        *degree -= 1;
                        if *degree == 0 {
                            ready.insert(*dependent);
                        }
                    }
                }
            }
        }

        if order.len() != self.tasks.len() {
            let members = find_cycle(&self.tasks).unwrap_or_default();
            return Err(GraphError::CycleDetected(members));
        }
        Ok(order)
    }

    fn check_single(&self, task: &Task) -> Result<(), GraphError> {
        if self.tasks.contains_key(&task.id) {
            return Err(GraphError::DuplicateId(task.id));
        }
        for dep in &task.deps {
            if *dep == task.id {
                return Err(GraphError::SelfDependency(task.id));
            }
            if !self.tasks.contains_key(dep) {
                return Err(GraphError::MissingDependency {
                    task: task.id,
                    dep: *dep,
                });
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mark {
    Unvisited,
    InProgress,
    Done,
}

/// Depth-first search over the dependency edges, returning the members of the
/// first cycle found (sorted ascending). Self dependencies and missing
/// dependencies are deliberately ignored here: they are reported by
/// [`TaskGraph::validate`] as dedicated issues.
fn find_cycle(tasks: &BTreeMap<TaskId, Task>) -> Option<Vec<TaskId>> {
    let mut marks: BTreeMap<TaskId, Mark> =
        tasks.keys().map(|id| (*id, Mark::Unvisited)).collect();
    let roots: Vec<TaskId> = tasks.keys().copied().collect();
    let mut path: Vec<TaskId> = Vec::new();
    for root in roots {
        if marks.get(&root) == Some(&Mark::Unvisited) {
            if let Some(members) = visit(tasks, root, &mut marks, &mut path) {
                return Some(members);
            }
        }
    }
    None
}

fn visit(
    tasks: &BTreeMap<TaskId, Task>,
    node: TaskId,
    marks: &mut BTreeMap<TaskId, Mark>,
    path: &mut Vec<TaskId>,
) -> Option<Vec<TaskId>> {
    marks.insert(node, Mark::InProgress);
    path.push(node);

    if let Some(task) = tasks.get(&node) {
        for dep in &task.deps {
            if *dep == node || !tasks.contains_key(dep) {
                continue;
            }
            match marks.get(dep).copied().unwrap_or(Mark::Unvisited) {
                Mark::Unvisited => {
                    if let Some(members) = visit(tasks, *dep, marks, path) {
                        return Some(members);
                    }
                }
                Mark::InProgress => {
                    let start = path.iter().position(|id| id == dep).unwrap_or(0);
                    let mut members = path[start..].to_vec();
                    members.sort_unstable();
                    return Some(members);
                }
                Mark::Done => {}
            }
        }
    }

    path.pop();
    marks.insert(node, Mark::Done);
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    // Contract C2 — a rejected operation leaves the graph unchanged.
    #[test]
    fn rejected_operation_leaves_graph_unchanged() {
        let mut graph = TaskGraph::new();
        graph.add_task(Task::new(1, 0)).unwrap();
        graph.add_task(Task::with_deps(2, 0, &[1])).unwrap();
        let before = graph.clone();

        // duplicate identifier
        assert_eq!(
            graph.add_task(Task::new(2, 5)).unwrap_err(),
            GraphError::DuplicateId(2)
        );
        assert_eq!(graph, before, "duplicate insertion must not touch the graph");

        // missing dependency
        assert_eq!(
            graph.add_task(Task::with_deps(3, 0, &[1, 8])).unwrap_err(),
            GraphError::MissingDependency { task: 3, dep: 8 }
        );
        assert_eq!(graph, before, "missing dependency must not touch the graph");

        // self dependency
        assert_eq!(
            graph.add_task(Task::with_deps(4, 0, &[4])).unwrap_err(),
            GraphError::SelfDependency(4)
        );
        assert_eq!(graph, before, "self dependency must not touch the graph");

        // a rejected batch must not insert its valid head either
        assert_eq!(
            graph
                .add_tasks(&[Task::new(5, 0), Task::with_deps(6, 0, &[42])])
                .unwrap_err(),
            GraphError::MissingDependency { task: 6, dep: 42 }
        );
        assert_eq!(graph, before, "batch must be atomic");
    }

    // Contract C3 — transitive cycles are never accepted.
    #[test]
    fn add_task_rejects_transitive_cycle() {
        // An inconsistent graph is imported: 1 <- 2 <- 3 <- 1.
        let mut graph = TaskGraph::from_tasks(&[
            Task::with_deps(1, 0, &[2]),
            Task::with_deps(2, 0, &[3]),
            Task::with_deps(3, 0, &[1]),
        ]);
        let err = graph.add_task(Task::new(4, 0)).unwrap_err();
        assert_eq!(err, GraphError::CycleDetected(vec![1, 2, 3]));
        assert_eq!(graph.len(), 3);
        assert!(!graph.contains(4));

        // A single self-edge is reported as such, before any cycle analysis.
        let mut clean = TaskGraph::new();
        clean.add_task(Task::new(1, 0)).unwrap();
        assert_eq!(
            clean.add_task(Task::with_deps(2, 0, &[2])).unwrap_err(),
            GraphError::SelfDependency(2)
        );
        assert_eq!(clean.len(), 1);

        // The same cycle submitted as a batch is rejected as a whole.
        let mut batch = TaskGraph::new();
        assert_eq!(
            batch
                .add_tasks(&[Task::with_deps(7, 0, &[8]), Task::with_deps(8, 0, &[9]), Task::with_deps(9, 0, &[7])])
                .unwrap_err(),
            GraphError::CycleDetected(vec![7, 8, 9])
        );
        assert!(batch.is_empty());
    }

    // Contract C4 — a single insertion cannot forward reference.
    #[test]
    fn add_task_rejects_missing_dependency() {
        let mut graph = TaskGraph::new();
        graph.add_task(Task::new(1, 0)).unwrap();
        assert_eq!(
            graph.add_task(Task::with_deps(2, 0, &[1, 3])).unwrap_err(),
            GraphError::MissingDependency { task: 2, dep: 3 }
        );
        assert!(!graph.contains(2));
        assert_eq!(graph.ids(), vec![1]);

        // Once the dependency exists, the very same task is accepted.
        graph.add_task(Task::new(3, 0)).unwrap();
        graph.add_task(Task::with_deps(2, 0, &[1, 3])).unwrap();
        assert_eq!(graph.ids(), vec![1, 2, 3]);
    }

    // Contract C5 — identifiers are unique.
    #[test]
    fn add_task_rejects_duplicate_id() {
        let mut graph = TaskGraph::new();
        graph.add_task(Task::new(7, 9)).unwrap();
        let before = graph.clone();
        assert_eq!(
            graph.add_task(Task::new(7, 1)).unwrap_err(),
            GraphError::DuplicateId(7)
        );
        assert_eq!(graph, before);
        // the priority of the rejected task must not have leaked
        assert_eq!(graph.get(7).unwrap().priority, 9);
        assert_eq!(graph.get(7).unwrap().seq, 0);
    }

    // Contract C6 — validate reports every anomaly, deterministically.
    #[test]
    fn validate_reports_all_anomalies_deterministically() {
        let mut duplicated = Task::with_deps(3, 0, &[4]);
        duplicated.deps.push(4);

        let mut graph = TaskGraph::from_tasks(&[
            Task::with_deps(1, 0, &[1]),   // self dependency
            Task::with_deps(2, 0, &[99]),  // unknown dependency
            duplicated,                    // duplicate dependency + cycle with 4
            Task::with_deps(4, 0, &[3]),
        ]);

        let expected = vec![
            ValidationIssue::SelfDependency { task: 1 },
            ValidationIssue::DuplicateDependency { task: 3, dep: 4 },
            ValidationIssue::MissingDependency { task: 2, dep: 99 },
            ValidationIssue::Cycle {
                members: vec![3, 4],
            },
        ];

        let first = graph.validate();
        assert_eq!(first, expected);
        // Stability: repeated calls and a rebuilt graph give the same report.
        assert_eq!(graph.validate(), first);
        assert_eq!(graph.validate(), graph.validate());

        // A valid graph reports nothing.
        graph.tasks.remove(&2);
        assert!(graph.validate().iter().all(|issue| {
            !matches!(issue, ValidationIssue::MissingDependency { .. })
        }));

        let mut clean = TaskGraph::new();
        clean.add_task(Task::new(10, 0)).unwrap();
        clean.add_task(Task::with_deps(11, 0, &[10])).unwrap();
        assert!(clean.validate().is_empty());
    }

    // Contract C7 — batches are atomic and allow forward references.
    #[test]
    fn add_tasks_is_atomic_and_allows_forward_references() {
        let mut graph = TaskGraph::new();
        graph.add_task(Task::new(1, 0)).unwrap();

        // 3 depends on 2, declared *after* it inside the same batch.
        graph
            .add_tasks(&[Task::with_deps(3, 0, &[2]), Task::with_deps(2, 0, &[1])])
            .unwrap();
        assert_eq!(graph.len(), 3);
        assert_eq!(graph.topo_order().unwrap(), vec![1, 2, 3]);
        // `seq` follows the declaration order of the batch.
        assert_eq!(graph.get(3).unwrap().seq, 1);
        assert_eq!(graph.get(2).unwrap().seq, 2);

        let before = graph.clone();

        // One bad task rejects the whole batch (even the valid head).
        assert_eq!(
            graph
                .add_tasks(&[Task::new(4, 0), Task::with_deps(5, 0, &[77])])
                .unwrap_err(),
            GraphError::MissingDependency { task: 5, dep: 77 }
        );
        assert_eq!(graph, before);

        // A cycle spanning the batch is rejected as a whole too.
        assert_eq!(
            graph
                .add_tasks(&[Task::with_deps(6, 0, &[7]), Task::with_deps(7, 0, &[6])])
                .unwrap_err(),
            GraphError::CycleDetected(vec![6, 7])
        );
        assert_eq!(graph, before);

        // Duplicates against the existing graph are rejected as well.
        assert_eq!(
            graph.add_tasks(&[Task::new(8, 0), Task::new(1, 0)]).unwrap_err(),
            GraphError::DuplicateId(1)
        );
        assert_eq!(graph, before);
    }

    #[test]
    fn topo_order_and_dependents_are_deterministic() {
        let mut graph = TaskGraph::new();
        graph
            .add_tasks(&[
                Task::with_deps(4, 0, &[2, 3]),
                Task::new(1, 0),
                Task::with_deps(2, 0, &[1]),
                Task::with_deps(3, 0, &[1]),
            ])
            .unwrap();

        assert_eq!(graph.topo_order().unwrap(), vec![1, 2, 3, 4]);
        assert_eq!(graph.topo_order().unwrap(), graph.topo_order().unwrap());
        assert_eq!(graph.dependents(1), vec![2, 3]);
        assert_eq!(graph.dependents(4), Vec::<TaskId>::new());
        assert_eq!(graph.ids(), vec![1, 2, 3, 4]);
    }
}
