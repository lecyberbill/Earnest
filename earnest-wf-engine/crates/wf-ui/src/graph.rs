use std::collections::HashMap;

use wf_core::ast::{ActionSpec, StepSpec, TriggerSpec, WorkflowSpec};
use wf_core::runtime::StepLog;
use ui_widgets::{GraphConnectionSpec, GraphNodeSpec, GraphSocket, SocketType};

const NODE_W: f32 = 220.0;
const NODE_H: f32 = 92.0;
const STEP_X: f32 = 270.0;
const STEP_Y: f32 = 130.0;

fn status_color(status: Option<&str>) -> [f32; 4] {
    match status {
        Some("success") => [0.06, 0.72, 0.50, 0.92], // Earnest Emerald
        Some("dry_run") => [0.02, 0.71, 0.83, 0.92], // Earnest Cyan
        Some("failed") => [0.95, 0.25, 0.37, 0.92],  // Earnest Rose
        _ => [0.38, 0.40, 0.94, 0.88],               // Earnest Indigo
    }
}

fn action_label(action: &ActionSpec) -> &'static str {
    match action {
        ActionSpec::VfsList { .. } => "vfs.list",
        ActionSpec::VfsRead { .. } => "vfs.read",
        ActionSpec::VfsWrite { .. } => "vfs.write",
        ActionSpec::VfsTransfer { .. } => "vfs.transfer",
        ActionSpec::VfsDelete { .. } => "vfs.delete",
        ActionSpec::VfsStat { .. } => "vfs.stat",
        ActionSpec::MailSend { .. } => "mail.send",
        ActionSpec::MailFetch { .. } => "mail.fetch",
        ActionSpec::FlowIf { .. } => "flow.if",
        ActionSpec::FlowEach { .. } => "flow.each",
        ActionSpec::FlowSet { .. } => "flow.set",
        ActionSpec::FlowSleep { .. } => "flow.sleep",
        ActionSpec::FlowCall { .. } => "flow.call",
        ActionSpec::FlowFail { .. } => "flow.fail",
        ActionSpec::SysExec { .. } => "sys.exec",
        ActionSpec::SysHttp { .. } => "sys.http",
        ActionSpec::SysLog { .. } => "sys.log",
    }
}

fn trigger_label(triggers: &[TriggerSpec]) -> String {
    match triggers.first() {
        Some(TriggerSpec::Manual) | None => "manual".to_string(),
        Some(TriggerSpec::Cron { expr, .. }) => format!("cron: {}", expr),
        Some(TriggerSpec::VfsWatch { path, .. }) => format!("vfs_watch: {}", path),
        Some(TriggerSpec::MailWatch { folder, .. }) => format!("mail_watch: {}", folder),
    }
}

/// Convertit un `WorkflowSpec` (+ `StepLog` optionnels) en graphe de nœuds AORUI.
pub fn workflow_to_graph(
    spec: &WorkflowSpec,
    logs: &[StepLog],
    selected_node: Option<&str>,
) -> (Vec<GraphNodeSpec>, Vec<GraphConnectionSpec>) {
    let mut status_by_step: HashMap<&str, &str> = HashMap::new();
    for log in logs {
        status_by_step.insert(log.step_id.as_str(), log.status.as_str());
    }

    let mut nodes = Vec::new();
    let edges = Vec::new();

    nodes.push(GraphNodeSpec {
        id: "__trigger__".to_string(),
        title: "TRIGGER".to_string(),
        subtitle: Some(trigger_label(&spec.triggers)),
        pos: [40.0, 140.0],
        size: [190.0, 84.0],
        inputs: vec![],
        outputs: vec![GraphSocket {
            name: "start".to_string(),
            socket_type: SocketType::Signal,
            color: None,
            is_output: true,
        }],
        header_color: Some([0.08, 0.55, 0.72, 0.92]),
        selected: selected_node == Some("__trigger__"),
    });

    let mut builder = GraphBuilder {
        status_by_step: &status_by_step,
        selected_node,
        nodes,
        edges,
    };
    builder.layout_sequence(&spec.steps, "__trigger__", 0, 300.0, 140.0);

    (builder.nodes, builder.edges)
}

struct GraphBuilder<'a> {
    status_by_step: &'a HashMap<&'a str, &'a str>,
    selected_node: Option<&'a str>,
    nodes: Vec<GraphNodeSpec>,
    edges: Vec<GraphConnectionSpec>,
}

impl<'a> GraphBuilder<'a> {
    fn layout_sequence(
        &mut self,
        steps: &'a [StepSpec],
        mut prev_id: &'a str,
        mut prev_socket: usize,
        start_x: f32,
        y: f32,
    ) {
        for (idx, step) in steps.iter().enumerate() {
            let x = start_x + (idx as f32) * STEP_X;
            let st = self.status_by_step.get(step.id.as_str()).copied();
            let subtitle = match st {
                Some(s) => format!("{} [{}]", action_label(&step.action), s),
                None => action_label(&step.action).to_string(),
            };

            let mut outputs = vec![GraphSocket {
                name: "next".to_string(),
                socket_type: SocketType::Flow,
                color: None,
                is_output: true,
            }];
            if matches!(step.action, ActionSpec::FlowIf { .. } | ActionSpec::FlowEach { .. }) {
                outputs.push(GraphSocket {
                    name: "branch".to_string(),
                    socket_type: SocketType::Data,
                    color: None,
                    is_output: true,
                });
            }

            self.nodes.push(GraphNodeSpec {
                id: step.id.clone(),
                title: step.name.clone().unwrap_or_else(|| step.id.clone()),
                subtitle: Some(subtitle),
                pos: [x, y],
                size: [NODE_W, NODE_H],
                inputs: vec![GraphSocket {
                    name: "in".to_string(),
                    socket_type: SocketType::Flow,
                    color: None,
                    is_output: false,
                }],
                outputs,
                header_color: Some(status_color(st)),
                selected: self.selected_node == Some(step.id.as_str()),
            });

            self.edges.push(GraphConnectionSpec {
                from_node: prev_id.to_string(),
                from_socket: prev_socket,
                to_node: step.id.clone(),
                to_socket: 0,
                color: None,
                flow_active: matches!(st, Some("success") | Some("dry_run")),
            });

            match &step.action {
                ActionSpec::FlowIf {
                    then, else_branch, ..
                } => {
                    if !then.is_empty() {
                        self.layout_sequence(then, &step.id, 1, x + 60.0, y + STEP_Y);
                    }
                    if !else_branch.is_empty() {
                        self.layout_sequence(
                            else_branch,
                            &step.id,
                            1,
                            x + 60.0,
                            y + STEP_Y * 2.0,
                        );
                    }
                }
                ActionSpec::FlowEach { steps: sub, .. } if !sub.is_empty() => {
                    self.layout_sequence(sub, &step.id, 1, x + 60.0, y + STEP_Y);
                }
                _ => {}
            }

            prev_id = &step.id;
            prev_socket = 0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wf_core::parse_workflow_yaml;

    #[test]
    fn builds_graph_nodes_and_edges_with_status_colors() {
        let yaml = r#"
version: "0.1"
name: "demo"
steps:
  - id: s1
    action: sys.log
    params:
      message: "ok"
  - id: cond
    action: flow.if
    condition: "true"
    then:
      - id: s2
        action: sys.log
        params:
          message: "then"
"#;
        let spec = parse_workflow_yaml(yaml).unwrap();
        let logs = vec![StepLog {
            step_id: "s1".to_string(),
            status: "success".to_string(),
            attempt: 1,
            output: None,
            error: None,
        }];
        let (nodes, edges) = workflow_to_graph(&spec, &logs, Some("cond"));
        assert_eq!(nodes.len(), 4); // trigger + s1 + cond + s2
        assert_eq!(edges.len(), 3);
        let s1 = nodes.iter().find(|n| n.id == "s1").unwrap();
        assert_eq!(s1.header_color, Some([0.06, 0.72, 0.50, 0.92]));
        assert!(nodes.iter().find(|n| n.id == "cond").unwrap().selected);
    }
}
