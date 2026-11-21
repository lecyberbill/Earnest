mod graph;

use std::path::PathBuf;
use std::sync::Arc;

use ui_core::UiEvent;
use ui_gpu::{BackgroundParams, GpuRenderer, RenderLayer, ResourceTable};
use ui_layout::{
    length, AlignItems, AvailableSpace, FlexDirection, JustifyContent, NodeId, Rect, Size, Style,
};
use ui_widgets::{
    ButtonVariant, GraphConnectionSpec, GraphNodeSpec, InteractionKey, InteractionState, Theme,
    WidgetTree,
};
use wf_core::runtime::StepLog;
use wf_core::{parse_workflow_yaml, ExecutionOptions, WfStore, WorkflowRecord};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowAttributes, WindowId};

const WINDOW_W: f32 = 1480.0;
const WINDOW_H: f32 = 900.0;

fn leaf(w: f32, h: f32) -> Style {
    Style {
        size: Size {
            width: length(w),
            height: length(h),
        },
        ..Default::default()
    }
}

fn row(gap: f32) -> Style {
    Style {
        flex_direction: FlexDirection::Row,
        align_items: Some(AlignItems::Center),
        gap: Size {
            width: length(gap),
            height: length(0.0),
        },
        ..Default::default()
    }
}

fn col(gap: f32) -> Style {
    Style {
        flex_direction: FlexDirection::Column,
        gap: Size {
            width: length(0.0),
            height: length(gap),
        },
        ..Default::default()
    }
}

struct StudioApp {
    store: WfStore,
    workflows: Vec<WorkflowRecord>,
    selected_idx: usize,
    yaml_text: String,
    nodes: Vec<GraphNodeSpec>,
    edges: Vec<GraphConnectionSpec>,
    last_logs: Vec<StepLog>,
    selected_node: Option<String>,
    status_line: String,
    pan: [f32; 2],
    zoom: f32,

    window: Option<Arc<Window>>,
    renderer: Option<GpuRenderer>,
    resources: ResourceTable,
    tree: WidgetTree,
    root: Option<NodeId>,
    theme: Theme,
    pressed: Option<InteractionKey>,
    cursor_pos: (f32, f32),
}

impl StudioApp {
    fn new(store: WfStore) -> Self {
        let theme = Theme::cyber_glass();

        let mut app = Self {
            store,
            workflows: Vec::new(),
            selected_idx: 0,
            yaml_text: String::new(),
            nodes: Vec::new(),
            edges: Vec::new(),
            last_logs: Vec::new(),
            selected_node: None,
            status_line: "Earnest WFL Studio prêt.".to_string(),
            pan: [0.0, 0.0],
            zoom: 1.0,
            window: None,
            renderer: None,
            resources: ResourceTable::new(),
            tree: WidgetTree::new(),
            root: None,
            theme,
            pressed: None,
            cursor_pos: (0.0, 0.0),
        };
        app.reload_workflows();
        app
    }

    fn reload_workflows(&mut self) {
        match self.store.list_workflows() {
            Ok(list) => {
                self.workflows = list;
                if self.selected_idx >= self.workflows.len() {
                    self.selected_idx = 0;
                }
                self.load_selected_workflow();
            }
            Err(e) => {
                self.status_line = format!("Erreur SQLite: {}", e);
            }
        }
    }

    fn load_selected_workflow(&mut self) {
        let Some(rec) = self.workflows.get(self.selected_idx).cloned() else {
            self.yaml_text = "# Aucun workflow enregistré dans .wf/wf.db".to_string();
            self.nodes.clear();
            self.edges.clear();
            return;
        };
        match std::fs::read_to_string(&rec.file_path) {
            Ok(yaml) => {
                self.yaml_text = yaml.clone();
                match parse_workflow_yaml(&yaml) {
                    Ok(spec) => {
                        let (nodes, edges) = graph::workflow_to_graph(
                            &spec,
                            &self.last_logs,
                            self.selected_node.as_deref(),
                        );
                        self.nodes = nodes;
                        self.edges = edges;
                    }
                    Err(e) => {
                        self.status_line = format!("YAML invalide: {}", e);
                    }
                }
            }
            Err(e) => {
                self.yaml_text = format!("# Lecture impossible: {}", e);
            }
        }
    }

    fn run_selected(&mut self, dry_run: bool) {
        let Some(rec) = self.workflows.get(self.selected_idx).cloned() else {
            return;
        };
        match self.store.execute_and_record(
            &rec.name,
            ExecutionOptions {
                dry_run,
                ..Default::default()
            },
        ) {
            Ok((run_id, report)) => {
                self.last_logs = report.logs;
                self.status_line = format!(
                    "{} ({}) -> {} ({} steps)",
                    run_id,
                    if dry_run { "dry_run" } else { "real" },
                    if report.success { "SUCCESS" } else { "FAILED" },
                    self.last_logs.len()
                );
                self.load_selected_workflow();
            }
            Err(e) => {
                self.status_line = format!("Bloqué: {}", e);
            }
        }
    }

    fn seal_selected(&mut self) {
        let Some(rec) = self.workflows.get(self.selected_idx).cloned() else {
            return;
        };
        match self.store.seal_workflow(&rec.name) {
            Ok(updated) => {
                self.status_line = format!(
                    "Scellé '{}' (SHA-256: {}...)",
                    updated.name,
                    &updated.sha256[..12.min(updated.sha256.len())]
                );
                self.reload_workflows();
            }
            Err(e) => {
                self.status_line = format!("Erreur scellage: {}", e);
            }
        }
    }

    fn handle_click(&mut self, x: f32, y: f32) {
        let Some(root) = self.root else { return };
        if let Ok(Some(UiEvent::ButtonClicked { widget_id })) = self.tree.dispatch_click(root, (x, y))
        {
            if let Some(idx_str) = widget_id.strip_prefix("wf_") {
                if let Ok(idx) = idx_str.parse::<usize>() {
                    self.selected_idx = idx;
                    self.last_logs.clear();
                    self.load_selected_workflow();
                }
            } else {
                match widget_id.as_str() {
                    "btn_dry_run" => self.run_selected(true),
                    "btn_seal" => self.seal_selected(),
                    "btn_real_run" => self.run_selected(false),
                    "btn_refresh" => self.reload_workflows(),
                    _ => {}
                }
            }
        }
    }

    fn render_frame(&mut self) {
        let Some(renderer) = self.renderer.as_mut() else {
            return;
        };
        let Some(window) = self.window.as_ref() else {
            return;
        };
        let phys = window.inner_size();
        let w = phys.width.max(1) as f32;
        let h = phys.height.max(1) as f32;

        let mut tree = WidgetTree::new();

        // 1. Top HUD (Earnest Operations Center style)
        let title_lbl = tree
            .label("Earnest • WFL Autonomous Studio", leaf(300.0, 24.0))
            .unwrap();
        let active_badge_txt = self
            .workflows
            .get(self.selected_idx)
            .map(|w| {
                format!(
                    "{} [{}] {}",
                    w.name,
                    w.status.to_uppercase(),
                    &w.sha256[..10.min(w.sha256.len())]
                )
            })
            .unwrap_or_else(|| "Aucun workflow".to_string());
        let badge_lbl = tree.label_muted(active_badge_txt, leaf(360.0, 22.0)).unwrap();

        let btn_dry = tree
            .button_variant(
                "btn_dry_run",
                "Dry-Run",
                ButtonVariant::Secondary,
                true,
                leaf(96.0, 32.0),
            )
            .unwrap();
        let btn_seal = tree
            .button_variant(
                "btn_seal",
                "Seal SHA-256",
                ButtonVariant::Primary,
                true,
                leaf(120.0, 32.0),
            )
            .unwrap();
        let btn_real = tree
            .button_variant(
                "btn_real_run",
                "Run (Real)",
                ButtonVariant::Primary,
                true,
                leaf(104.0, 32.0),
            )
            .unwrap();
        let btn_ref = tree
            .button_variant(
                "btn_refresh",
                "Refresh",
                ButtonVariant::Ghost,
                true,
                leaf(84.0, 32.0),
            )
            .unwrap();

        let mut hud_style = row(10.0);
        hud_style.size = Size {
            width: length(w - 24.0),
            height: length(50.0),
        };
        hud_style.justify_content = Some(JustifyContent::SpaceBetween);
        hud_style.padding = Rect {
            left: length(14.0),
            right: length(14.0),
            top: length(8.0),
            bottom: length(8.0),
        };
        let hud_left = tree.container(&[title_lbl, badge_lbl], row(12.0)).unwrap();
        let hud_right = tree
            .container(&[btn_dry, btn_seal, btn_real, btn_ref], row(8.0))
            .unwrap();
        let hud_panel = tree.panel(&[hud_left, hud_right], None, None, hud_style).unwrap();

        // 2. Left Sidebar: Workflows list
        let wf_header = tree.label("Workflows (.wf/wf.db)", leaf(220.0, 22.0)).unwrap();
        let mut wf_items = vec![wf_header];
        for (idx, wf) in self.workflows.iter().enumerate() {
            let label = format!("{} [{}]", wf.name, wf.status);
            let variant = if idx == self.selected_idx {
                ButtonVariant::Primary
            } else {
                ButtonVariant::Ghost
            };
            let btn = tree
                .button_variant(format!("wf_{}", idx), label, variant, true, leaf(224.0, 32.0))
                .unwrap();
            wf_items.push(btn);
        }
        let mut left_style = col(6.0);
        left_style.size = Size {
            width: length(250.0),
            height: length(h - 110.0),
        };
        left_style.padding = Rect {
            left: length(12.0),
            right: length(12.0),
            top: length(12.0),
            bottom: length(12.0),
        };
        let left_panel = tree.panel(&wf_items, None, None, left_style).unwrap();

        // 3. Center Canvas: AORUI NodeGraph
        let graph_w = (w - 250.0 - 400.0 - 48.0).max(320.0);
        let graph_h = (h - 110.0).max(320.0);
        let graph_node = tree
            .node_graph(
                "wfl_graph",
                self.nodes.clone(),
                self.edges.clone(),
                self.pan,
                self.zoom,
                None,
                leaf(graph_w, graph_h),
            )
            .unwrap();

        // 4. Right Inspector: YAML + Step Logs
        let yaml_title = tree.label("YAML & Step Logs", leaf(360.0, 22.0)).unwrap();
        let yaml_ed = tree
            .code_editor(
                "yaml_preview",
                self.yaml_text.clone(),
                "yaml",
                true,
                false,
                0,
                None,
                leaf(374.0, (h - 170.0).max(200.0)),
            )
            .unwrap();
        let mut right_style = col(8.0);
        right_style.size = Size {
            width: length(400.0),
            height: length(h - 110.0),
        };
        right_style.padding = Rect {
            left: length(12.0),
            right: length(12.0),
            top: length(12.0),
            bottom: length(12.0),
        };
        let right_panel = tree.panel(&[yaml_title, yaml_ed], None, None, right_style).unwrap();

        let body = tree
            .container(&[left_panel, graph_node, right_panel], row(12.0))
            .unwrap();
        let status_lbl = tree.label_muted(self.status_line.clone(), leaf(w - 32.0, 20.0)).unwrap();

        let mut root_style = col(10.0);
        root_style.size = Size {
            width: length(w),
            height: length(h),
        };
        root_style.padding = Rect {
            left: length(12.0),
            right: length(12.0),
            top: length(10.0),
            bottom: length(10.0),
        };
        let root = tree.container(&[hud_panel, body, status_lbl], root_style).unwrap();

        let avail = Size {
            width: AvailableSpace::Definite(w),
            height: AvailableSpace::Definite(h),
        };
        tree.compute(root, avail).unwrap();

        let hovered = tree.interaction_key_at(root, self.cursor_pos).unwrap_or(None);
        let measure = renderer.text_measure();
        let interaction = InteractionState {
            hovered: hovered.as_ref(),
            pressed: self.pressed.as_ref(),
            measure: Some(&measure),
        };
        let frame = tree.build_frame(root, &self.theme, interaction).unwrap();

        let text_runs: Vec<_> = frame
            .texts
            .iter()
            .map(|spec| {
                let family = glyphon::Family::SansSerif;
                let align = match spec.align {
                    ui_widgets::TextAlign::Left => glyphon::cosmic_text::Align::Left,
                    ui_widgets::TextAlign::Center => glyphon::cosmic_text::Align::Center,
                    ui_widgets::TextAlign::Right => glyphon::cosmic_text::Align::Right,
                };
                let weight = match spec.weight {
                    ui_widgets::FontWeight::Normal => glyphon::Weight::NORMAL,
                    ui_widgets::FontWeight::Bold => glyphon::Weight::BOLD,
                };
                renderer.make_text_run(
                    &spec.text,
                    spec.bounds,
                    spec.font_size,
                    spec.color,
                    align,
                    family,
                    weight,
                    spec.clip,
                )
            })
            .collect();

        let layer = RenderLayer {
            instances: &frame.instances,
            texts: &text_runs,
        };
        let bg = wgpu::Color {
            r: 0.012,
            g: 0.020,
            b: 0.045,
            a: 1.0,
        };
        if let Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) =
            renderer.render_layers(bg, &[layer], &[], &self.resources)
        {
            renderer.resize(phys);
        }

        self.tree = tree;
        self.root = Some(root);
    }
}

impl ApplicationHandler for StudioApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attrs = WindowAttributes::default()
            .with_title("Earnest • WFL Autonomous Studio (AORUI)")
            .with_inner_size(winit::dpi::PhysicalSize::new(WINDOW_W as u32, WINDOW_H as u32));
        let window = Arc::new(event_loop.create_window(attrs).unwrap());
        let mut renderer = GpuRenderer::new(window.clone());
        let mut bg = BackgroundParams::aether_os(WINDOW_W, WINDOW_H);
        bg.base_color = [0.008, 0.014, 0.030, 1.0];
        bg.grad1_color = [0.096, 0.106, 0.718, 0.18];
        bg.grad2_color = [0.010, 0.055, 0.140, 0.14];
        renderer.set_background_params(bg);
        self.window = Some(window);
        self.renderer = Some(renderer);
        event_loop.set_control_flow(ControlFlow::Wait);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(r) = self.renderer.as_mut() {
                    r.resize(size);
                }
                if let Some(w) = self.window.as_ref() {
                    w.request_redraw();
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor_pos = (position.x as f32, position.y as f32);
                if let Some(w) = self.window.as_ref() {
                    w.request_redraw();
                }
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => {
                self.pressed = self
                    .root
                    .and_then(|r| self.tree.interaction_key_at(r, self.cursor_pos).ok().flatten());
                self.handle_click(self.cursor_pos.0, self.cursor_pos.1);
                if let Some(w) = self.window.as_ref() {
                    w.request_redraw();
                }
            }
            WindowEvent::MouseInput {
                state: ElementState::Released,
                button: MouseButton::Left,
                ..
            } => {
                self.pressed = None;
                if let Some(w) = self.window.as_ref() {
                    w.request_redraw();
                }
            }
            WindowEvent::RedrawRequested => self.render_frame(),
            _ => {}
        }
    }
}

fn main() {
    let mut workspace = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let mut db_path: Option<PathBuf> = None;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--workspace" => {
                if let Some(v) = args.next() {
                    workspace = PathBuf::from(v);
                }
            }
            "--db" => {
                if let Some(v) = args.next() {
                    db_path = Some(PathBuf::from(v));
                }
            }
            _ => {}
        }
    }

    let db = db_path.unwrap_or_else(|| workspace.join(".wf").join("wf.db"));
    let store = WfStore::open(&db).expect("Impossible d'ouvrir WfStore");

    let event_loop = EventLoop::new().expect("EventLoop error");
    let mut app = StudioApp::new(store);
    let _ = event_loop.run_app(&mut app);
}
