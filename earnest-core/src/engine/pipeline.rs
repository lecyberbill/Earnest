use std::collections::BTreeMap;

use agent_scheduler::{complete, fail, start_batch, ProgressSummary, Task, TaskGraph, TaskStatus};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize)]
pub struct PipelineTaskSpec {
    pub id: u64,
    pub title: String,
    pub prompt: String,
    #[serde(default = "default_priority")]
    pub priority: u8,
    #[serde(default)]
    pub depends_on: Vec<u64>,
}

fn default_priority() -> u8 {
    128
}

#[derive(Debug, Clone, Serialize)]
pub struct PipelineTaskOutcome {
    pub id: u64,
    pub title: String,
    pub status: String,
    pub output: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PipelineRunReport {
    pub total: usize,
    pub completed: usize,
    pub failed: usize,
    pub blocked: usize,
    pub progress_fraction: f64,
    pub tasks: Vec<PipelineTaskOutcome>,
}

/// Construit et valide un graphe DAG de tâches via `agent-scheduler`.
pub fn build_validated_graph(specs: &[PipelineTaskSpec]) -> Result<TaskGraph, String> {
    let mut graph = TaskGraph::new();
    let batch: Vec<Task> = specs
        .iter()
        .map(|s| Task {
            id: s.id,
            priority: s.priority,
            seq: 0,
            status: TaskStatus::Pending,
            deps: s.depends_on.clone(),
        })
        .collect();
    graph
        .add_tasks(&batch)
        .map_err(|e| format!("Graphe de pipeline invalide : {}", e))?;
    let issues = graph.validate();
    if !issues.is_empty() {
        return Err(format!("Validation DAG échouée : {:?}", issues));
    }
    Ok(graph)
}

/// Exécute un pipeline DAG de tâches avec un exécuteur injectable `runner(task, upstream_context)`.
pub fn execute_dag_pipeline<F>(
    specs: &[PipelineTaskSpec],
    max_concurrency: usize,
    mut runner: F,
) -> Result<PipelineRunReport, String>
where
    F: FnMut(&PipelineTaskSpec, &str) -> Result<String, String>,
{
    let mut graph = build_validated_graph(specs)?;
    let spec_map: BTreeMap<u64, &PipelineTaskSpec> = specs.iter().map(|s| (s.id, s)).collect();
    let mut outputs: BTreeMap<u64, String> = BTreeMap::new();
    let mut outcomes: Vec<PipelineTaskOutcome> = Vec::new();
    let concurrency = max_concurrency.max(1);

    loop {
        let batch = start_batch(&mut graph, concurrency)
            .map_err(|e| format!("Erreur ordonnancement batch : {}", e))?;
        if batch.is_empty() {
            break;
        }
        for tid in batch {
            let Some(spec) = spec_map.get(&tid) else {
                let _ = fail(&mut graph, tid);
                continue;
            };
            let mut ctx = String::new();
            for dep in &spec.depends_on {
                if let Some(dep_out) = outputs.get(dep) {
                    if let Some(dep_spec) = spec_map.get(dep) {
                        ctx.push_str(&format!("\n[Sortie étape {} - {}]:\n{}\n", dep, dep_spec.title, dep_out));
                    }
                }
            }
            match runner(spec, &ctx) {
                Ok(out) => {
                    let _ = complete(&mut graph, tid);
                    outputs.insert(tid, out.clone());
                    outcomes.push(PipelineTaskOutcome {
                        id: tid,
                        title: spec.title.clone(),
                        status: "completed".to_string(),
                        output: out,
                    });
                }
                Err(err) => {
                    let _ = fail(&mut graph, tid);
                    outcomes.push(PipelineTaskOutcome {
                        id: tid,
                        title: spec.title.clone(),
                        status: "failed".to_string(),
                        output: err,
                    });
                }
            }
        }
    }

    let summary = ProgressSummary::from_graph(&graph);
    let progress_fraction = if summary.total == 0 {
        1.0
    } else {
        (summary.completed + summary.failed + summary.cancelled) as f64 / summary.total as f64
    };
    Ok(PipelineRunReport {
        total: summary.total,
        completed: summary.completed,
        failed: summary.failed,
        blocked: summary.pending,
        progress_fraction,
        tasks: outcomes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dag_pipeline_rejects_cycles() {
        let specs = vec![
            PipelineTaskSpec {
                id: 1,
                title: "A".into(),
                prompt: "p".into(),
                priority: 10,
                depends_on: vec![2],
            },
            PipelineTaskSpec {
                id: 2,
                title: "B".into(),
                prompt: "p".into(),
                priority: 10,
                depends_on: vec![1],
            },
        ];
        assert!(build_validated_graph(&specs).is_err());
    }

    #[test]
    fn dag_pipeline_executes_in_dependency_and_priority_order() {
        let specs = vec![
            PipelineTaskSpec {
                id: 1,
                title: "Architect".into(),
                prompt: "design".into(),
                priority: 200,
                depends_on: vec![],
            },
            PipelineTaskSpec {
                id: 2,
                title: "Dev".into(),
                prompt: "implement".into(),
                priority: 150,
                depends_on: vec![1],
            },
            PipelineTaskSpec {
                id: 3,
                title: "QA".into(),
                prompt: "verify".into(),
                priority: 100,
                depends_on: vec![2],
            },
        ];
        let mut order = Vec::new();
        let report = execute_dag_pipeline(&specs, 2, |spec, ctx| {
            order.push(spec.id);
            Ok(format!("done:{}{}", spec.title, if ctx.is_empty() { "" } else { "+ctx" }))
        })
        .unwrap();
        assert_eq!(order, vec![1, 2, 3]);
        assert_eq!(report.completed, 3);
        assert_eq!(report.failed, 0);
        assert_eq!(report.blocked, 0);
        assert_eq!(report.tasks[1].output, "done:Dev+ctx");
    }

    #[test]
    fn dag_pipeline_blocks_dependents_on_failure() {
        let specs = vec![
            PipelineTaskSpec {
                id: 1,
                title: "Architect".into(),
                prompt: "design".into(),
                priority: 200,
                depends_on: vec![],
            },
            PipelineTaskSpec {
                id: 2,
                title: "Dev".into(),
                prompt: "implement".into(),
                priority: 150,
                depends_on: vec![1],
            },
        ];
        let report = execute_dag_pipeline(&specs, 2, |_, _| Err("boom".into())).unwrap();
        assert_eq!(report.completed, 0);
        assert_eq!(report.failed, 1);
        assert_eq!(report.blocked, 1);
    }
}
