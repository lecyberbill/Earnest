mod graph;
mod palette;

use std::path::PathBuf;
use std::sync::Arc;

use palette::{FormModalState, ACTION_PALETTE};
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
use wf_core::{
    parse_workflow_yaml, DebugSession, ExecutionOptions, RunSummary, WfStore, WorkflowRecord,
};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, KeyEvent, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowAttributes, WindowId};

const WINDOW_W: f32 = 1520.0;
const WINDOW_H: f32 = 940.0;

#[derive(Clone, Copy, PartialEq, Eq)]
enum StudioTab {
    Dashboard,
    Studio,
    Schedule,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum LeftPane {
    Workflows,
    Palette,
}

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
    workspace: PathBuf,
    store: WfStore,
    tab: StudioTab,
    left_pane: LeftPane,
    workflows: Vec<WorkflowRecord>,
    runs: Vec<RunSummary>,
    selected_idx: usize,
    yaml_text: String,
    nodes: Vec<GraphNodeSpec>,
    edges: Vec<GraphConnectionSpec>,
    last_logs: Vec<StepLog>,
    debug: Option<DebugSession>,
    selected_node: Option<String>,
    form: FormModalState,
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
    fn new(workspace: PathBuf, store: WfStore) -> Self {
        let theme = Theme::cyber_glass();
        let mut app = Self {
            workspace,
            store,
            tab: StudioTab::Studio,
            left_pane: LeftPane::Workflows,
            workflows: Vec::new(),
            runs: Vec::new(),
            selected_idx: 0,
            yaml_text: String::new(),
            nodes: Vec::new(),
            edges: Vec::new(),
            last_logs: Vec::new(),
            debug: None,
            selected_node: None,
            form: FormModalState::default(),
            status_line: "Earnest • WFL Studio prêt.".to_string(),
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
        app.reload_all();
        app
    }

    fn reload_all(&mut self) {
        let _ = self.store.scan_workflows_dir(&self.workspace.join("workflows"));
        match self.store.list_workflows() {
            Ok(list) => {
                self.workflows = list;
                if self.selected_idx >= self.workflows.len() {
                    self.selected_idx = 0;
                }
                self.runs = self.store.list_runs(None, 12).unwrap_or_default();
                self.load_selected_workflow();
            }
            Err(e) => self.status_line = format!("Erreur SQLite: {}", e),
        }
    }

    fn load_selected_workflow(&mut self) {
        let Some(rec) = self.workflows.get(self.selected_idx).cloned() else {
            self.yaml_text = "# Aucun workflow. Utilisez 'Ouvrir YAML...' ou la Palette.".to_string();
            self.nodes.clear();
            self.edges.clear();
            return;
        };
        match std::fs::read_to_string(&rec.file_path) {
            Ok(yaml) => {
                self.yaml_text = yaml.clone();
                if let Ok(spec) = parse_workflow_yaml(&yaml) {
                    let active = self
                        .debug
                        .as_ref()
                        .and_then(|d| d.current_step_id().map(String::from));
                    let sel = active.as_deref().or(self.selected_node.as_deref());
                    let (nodes, edges) = graph::workflow_to_graph(&spec, &self.last_logs, sel);
                    self.nodes = nodes;
                    self.edges = edges;
                }
            }
            Err(e) => self.yaml_text = format!("# Lecture impossible: {}", e),
        }
    }

    fn run_selected(&mut self, dry_run: bool) {
        let Some(rec) = self.workflows.get(self.selected_idx).cloned() else { return };
        self.debug = None;
        match self.store.execute_and_record(
            &rec.name,
            ExecutionOptions { dry_run, ..Default::default() },
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
                self.runs = self.store.list_runs(None, 12).unwrap_or_default();
                self.load_selected_workflow();
            }
            Err(e) => self.status_line = format!("Bloqué: {}", e),
        }
    }

    fn step_debug(&mut self) {
        if self.debug.is_none() {
            let Some(rec) = self.workflows.get(self.selected_idx).cloned() else { return };
            match std::fs::read_to_string(&rec.file_path).and_then(|y| parse_workflow_yaml(&y).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))) {
                Ok(spec) => {
                    match DebugSession::new(spec, ExecutionOptions { dry_run: true, ..Default::default() }) {
                        Ok(sess) => {
                            self.debug = Some(sess);
                            self.last_logs.clear();
                        }
                        Err(e) => {
                            self.status_line = format!("Debug init erreur: {}", e);
                            return;
                        }
                    }
                }
                Err(e) => {
                    self.status_line = format!("Erreur lecture: {}", e);
                    return;
                }
            }
        }
        if let Some(ref mut sess) = self.debug {
            if let Some(log) = sess.step() {
                self.last_logs = sess.logs().to_vec();
                self.status_line = format!(
                    "DEBUG [{}] -> {} {}",
                    log.step_id,
                    log.status,
                    if sess.is_finished() { "(TERMINÉ)" } else { "" }
                );
            } else {
                self.status_line = "Debug terminé. Cliquez Reset pour relancer.".to_string();
            }
        }
        self.load_selected_workflow();
    }

    fn reset_debug(&mut self) {
        self.debug = None;
        self.last_logs.clear();
        self.status_line = "Session de debug réinitialisée.".to_string();
        self.load_selected_workflow();
    }

    fn seal_selected(&mut self) {
        let Some(rec) = self.workflows.get(self.selected_idx).cloned() else { return };
        match self.store.seal_workflow(&rec.name) {
            Ok(updated) => {
                self.status_line = format!(
                    "Scellé '{}' ({}...)",
                    updated.name,
                    &updated.sha256[..12.min(updated.sha256.len())]
                );
                self.reload_all();
            }
            Err(e) => self.status_line = format!("Erreur scellage: {}", e),
        }
    }

    fn import_yaml_dialog(&mut self) {
        if let Some(path) = pick_yaml_file_dialog(&self.workspace) {
            match self.store.import_yaml_file(&path) {
                Ok(rec) => {
                    self.status_line = format!("Importé '{}' en draft.", rec.name);
                    self.reload_all();
                    if let Some(pos) = self.workflows.iter().position(|w| w.name == rec.name) {
                        self.selected_idx = pos;
                        self.load_selected_workflow();
                    }
                }
                Err(e) => self.status_line = format!("Erreur import YAML: {}", e),
            }
        }
    }

    fn choose_workspace(&mut self) {
        if let Some(dir) = pick_folder_dialog(&self.workspace) {
            let db = dir.join(".wf").join("wf.db");
            if let Ok(new_store) = WfStore::open(&db) {
                self.workspace = dir.clone();
                self.store = new_store;
                self.selected_idx = 0;
                self.debug = None;
                self.last_logs.clear();
                self.status_line = format!("Workspace : {}", dir.display());
                self.reload_all();
            }
        }
    }

    fn submit_form_step(&mut self) {
        let Some(rec) = self.workflows.get(self.selected_idx).cloned() else {
            self.status_line = "Sélectionnez ou importez d'abord un workflow.".to_string();
            self.form.open = false;
            return;
        };
        let step = match self.form.build_step() {
            Ok(s) => s,
            Err(e) => {
                self.status_line = e;
                return;
            }
        };
        let Ok(yaml) = std::fs::read_to_string(&rec.file_path) else { return };
        let Ok(mut spec) = parse_workflow_yaml(&yaml) else { return };
        spec.steps.push(step);
        match self.store.save_draft(&spec, std::path::Path::new(&rec.file_path)) {
            Ok(_) => {
                self.form.open = false;
                self.status_line = format!("Étape ajoutée à '{}' (statut draft).", rec.name);
                self.reload_all();
            }
            Err(e) => self.status_line = format!("Validation refusée: {}", e),
        }
    }

    fn toggle_workflow_enabled(&mut self, idx: usize) {
        if let Some(wf) = self.workflows.get(idx).cloned() {
            let next = !wf.enabled;
            let _ = self.store.set_workflow_enabled(&wf.name, next);
            self.status_line = format!("'{}' {}", wf.name, if next { "activé" } else { "désactivé" });
            self.reload_all();
        }
    }

    fn cycle_workflow_folder(&mut self, idx: usize) {
        const FOLDERS: &[&str] = &["General", "Operations", "Data/VFS", "Prod"];
        if let Some(wf) = self.workflows.get(idx).cloned() {
            let pos = FOLDERS.iter().position(|f| *f == wf.folder).unwrap_or(0);
            let next = FOLDERS[(pos + 1) % FOLDERS.len()];
            let _ = self.store.set_workflow_folder(&wf.name, next);
            self.status_line = format!("'{}' déplacé dans dossier [{}]", wf.name, next);
            self.reload_all();
        }
    }

    fn set_cron_preset(&mut self, idx: usize, expr: Option<&str>) {
        if let Some(wf) = self.workflows.get(idx).cloned() {
            let _ = self.store.set_workflow_cron(&wf.name, expr);
            self.status_line = match expr {
                Some(e) => format!("Cron '{}' -> {}", wf.name, e),
                None => format!("Cron '{}' désactivé", wf.name),
            };
            self.reload_all();
        }
    }

    fn handle_click(&mut self, x: f32, y: f32) {
        let Some(root) = self.root else { return };
        if let Ok(Some(UiEvent::ButtonClicked { widget_id })) = self.tree.dispatch_click(root, (x, y)) {
            if let Some(idx_s) = widget_id.strip_prefix("wf_") {
                if let Ok(idx) = idx_s.parse::<usize>() {
                    self.selected_idx = idx;
                    self.reset_debug();
                }
            } else if let Some(idx_s) = widget_id.strip_prefix("pal_") {
                if let Ok(idx) = idx_s.parse::<usize>() {
                    self.form.open_for(idx, self.nodes.len());
                }
            } else if let Some(idx_s) = widget_id.strip_prefix("tog_") {
                if let Ok(idx) = idx_s.parse::<usize>() {
                    self.toggle_workflow_enabled(idx);
                }
            } else if let Some(idx_s) = widget_id.strip_prefix("fld_") {
                if let Ok(idx) = idx_s.parse::<usize>() {
                    self.cycle_workflow_folder(idx);
                }
            } else if let Some(idx_s) = widget_id.strip_prefix("open_") {
                if let Ok(idx) = idx_s.parse::<usize>() {
                    self.selected_idx = idx;
                    self.tab = StudioTab::Studio;
                    self.reset_debug();
                }
            } else if let Some(idx_s) = widget_id.strip_prefix("cron5_") {
                if let Ok(idx) = idx_s.parse::<usize>() {
                    self.set_cron_preset(idx, Some("*/5 * * * *"));
                }
            } else if let Some(idx_s) = widget_id.strip_prefix("cron2h_") {
                if let Ok(idx) = idx_s.parse::<usize>() {
                    self.set_cron_preset(idx, Some("0 2 * * *"));
                }
            } else if let Some(idx_s) = widget_id.strip_prefix("cronoff_") {
                if let Ok(idx) = idx_s.parse::<usize>() {
                    self.set_cron_preset(idx, None);
                }
            } else if let Some(idx_s) = widget_id.strip_prefix("ff_") {
                if let Ok(fidx) = idx_s.parse::<usize>() {
                    self.form.active_field = fidx;
                }
            } else {
                match widget_id.as_str() {
                    "tab_dash" => self.tab = StudioTab::Dashboard,
                    "tab_studio" => self.tab = StudioTab::Studio,
                    "tab_sched" => self.tab = StudioTab::Schedule,
                    "left_wf" => self.left_pane = LeftPane::Workflows,
                    "left_pal" => self.left_pane = LeftPane::Palette,
                    "btn_open_ws" => self.choose_workspace(),
                    "btn_import_yaml" => self.import_yaml_dialog(),
                    "btn_dry_run" => self.run_selected(true),
                    "btn_step" => self.step_debug(),
                    "btn_reset_dbg" => self.reset_debug(),
                    "btn_seal" => self.seal_selected(),
                    "btn_real_run" => self.run_selected(false),
                    "btn_refresh" => self.reload_all(),
                    "form_submit" => self.submit_form_step(),
                    "form_cancel" => self.form.open = false,
                    _ => {}
                }
            }
        }
    }

    fn render_frame(&mut self) {
        let Some(window) = self.window.as_ref() else { return };
        let phys = window.inner_size();
        let w = phys.width.max(1) as f32;
        let h = phys.height.max(1) as f32;
        let mut tree = WidgetTree::new();

        // Top HUD
        let title = tree.label("Earnest • WFL Studio", leaf(200.0, 24.0)).unwrap();
        let t_dash = tree.button_variant("tab_dash", "Dashboard", if self.tab == StudioTab::Dashboard { ButtonVariant::Primary } else { ButtonVariant::Ghost }, true, leaf(96.0, 30.0)).unwrap();
        let t_stud = tree.button_variant("tab_studio", "Studio", if self.tab == StudioTab::Studio { ButtonVariant::Primary } else { ButtonVariant::Ghost }, true, leaf(80.0, 30.0)).unwrap();
        let t_sch = tree.button_variant("tab_sched", "Planif", if self.tab == StudioTab::Schedule { ButtonVariant::Primary } else { ButtonVariant::Ghost }, true, leaf(76.0, 30.0)).unwrap();

        let b_yaml = tree.button_variant("btn_import_yaml", "Ouvrir YAML", ButtonVariant::Secondary, true, leaf(104.0, 30.0)).unwrap();
        let b_dry = tree.button_variant("btn_dry_run", "Dry-Run", ButtonVariant::Secondary, true, leaf(82.0, 30.0)).unwrap();
        let b_step = tree.button_variant("btn_step", "Step >", ButtonVariant::Primary, true, leaf(74.0, 30.0)).unwrap();
        let b_rst = tree.button_variant("btn_reset_dbg", "Reset", ButtonVariant::Ghost, true, leaf(68.0, 30.0)).unwrap();
        let b_seal = tree.button_variant("btn_seal", "Seal", ButtonVariant::Secondary, true, leaf(66.0, 30.0)).unwrap();
        let b_real = tree.button_variant("btn_real_run", "Run", ButtonVariant::Primary, true, leaf(64.0, 30.0)).unwrap();
        let b_ref = tree.button_variant("btn_refresh", "Refresh", ButtonVariant::Ghost, true, leaf(76.0, 30.0)).unwrap();

        let hud_l = tree.container(&[title, t_dash, t_stud, t_sch], row(8.0)).unwrap();
        let hud_r = tree.container(&[b_yaml, b_dry, b_step, b_rst, b_seal, b_real, b_ref], row(6.0)).unwrap();
        let mut hud_s = row(10.0);
        hud_s.size = Size { width: length(w - 24.0), height: length(48.0) };
        hud_s.justify_content = Some(JustifyContent::SpaceBetween);
        hud_s.padding = Rect { left: length(12.0), right: length(12.0), top: length(6.0), bottom: length(6.0) };
        let hud = tree.panel(&[hud_l, hud_r], None, None, hud_s).unwrap();

        let body_h = (h - 108.0).max(280.0);
        let body = match self.tab {
            StudioTab::Dashboard => self.build_dashboard(&mut tree, w - 24.0, body_h),
            StudioTab::Studio => self.build_studio(&mut tree, w - 24.0, body_h),
            StudioTab::Schedule => self.build_schedule(&mut tree, w - 24.0, body_h),
        };

        let status = tree.label_muted(self.status_line.clone(), leaf(w - 32.0, 20.0)).unwrap();
        let mut root_s = col(8.0);
        root_s.size = Size { width: length(w), height: length(h) };
        root_s.padding = Rect { left: length(12.0), right: length(12.0), top: length(8.0), bottom: length(8.0) };
        let mut children = vec![hud, body, status];
        if self.form.open {
            children.push(self.build_form_modal(&mut tree));
        }
        let root = tree.container(&children, root_s).unwrap();

        let avail = Size { width: AvailableSpace::Definite(w), height: AvailableSpace::Definite(h) };
        tree.compute(root, avail).unwrap();
        let hovered = tree.interaction_key_at(root, self.cursor_pos).unwrap_or(None);
        let Some(renderer) = self.renderer.as_mut() else { return };
        let measure = renderer.text_measure();
        let interaction = InteractionState { hovered: hovered.as_ref(), pressed: self.pressed.as_ref(), measure: Some(&measure) };
        let frame = tree.build_frame(root, &self.theme, interaction).unwrap();

        let text_runs: Vec<_> = frame.texts.iter().map(|spec| {
            let align = match spec.align {
                ui_widgets::TextAlign::Left => glyphon::cosmic_text::Align::Left,
                ui_widgets::TextAlign::Center => glyphon::cosmic_text::Align::Center,
                ui_widgets::TextAlign::Right => glyphon::cosmic_text::Align::Right,
            };
            let weight = match spec.weight {
                ui_widgets::FontWeight::Normal => glyphon::Weight::NORMAL,
                ui_widgets::FontWeight::Bold => glyphon::Weight::BOLD,
            };
            renderer.make_text_run(&spec.text, spec.bounds, spec.font_size, spec.color, align, glyphon::Family::SansSerif, weight, spec.clip)
        }).collect();

        let layer = RenderLayer { instances: &frame.instances, texts: &text_runs };
        let bg = wgpu::Color { r: 0.012, g: 0.020, b: 0.045, a: 1.0 };
        if let Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) = renderer.render_layers(bg, &[layer], &[], &self.resources) {
            renderer.resize(phys);
        }
        self.tree = tree;
        self.root = Some(root);
    }

    fn build_dashboard(&self, tree: &mut WidgetTree, w: f32, h: f32) -> NodeId {
        let total = self.workflows.len();
        let enabled_cnt = self.workflows.iter().filter(|x| x.enabled).count();
        let sealed_cnt = self.workflows.iter().filter(|x| x.status == "sealed").count();
        let ok_runs = self.runs.iter().filter(|r| r.status == "success").count();

        let k1_l = tree.label(format!("Flux totaux : {}", total), leaf(220.0, 28.0)).unwrap();
        let k1 = tree.panel(&[k1_l], None, None, leaf(250.0, 56.0)).unwrap();
        let k2_l = tree.label(format!("Actifs : {} | Scellés : {}", enabled_cnt, sealed_cnt), leaf(240.0, 28.0)).unwrap();
        let k2 = tree.panel(&[k2_l], None, None, leaf(270.0, 56.0)).unwrap();
        let k3_l = tree.label(format!("Runs récents : {} ({} OK)", self.runs.len(), ok_runs), leaf(240.0, 28.0)).unwrap();
        let k3 = tree.panel(&[k3_l], None, None, leaf(270.0, 56.0)).unwrap();
        let kpis = tree.container(&[k1, k2, k3], row(12.0)).unwrap();

        let mut wf_rows = vec![tree.label("Gestion des Flux (Activation / Dossiers / Statut)", leaf(600.0, 24.0)).unwrap()];
        for (idx, wf) in self.workflows.iter().enumerate() {
            let info = tree.label(format!("[{}] {}  •  {}  •  {}", wf.folder, wf.name, wf.status.to_uppercase(), if wf.enabled { "ACTIF" } else { "INACTIF" }), leaf(520.0, 26.0)).unwrap();
            let b_tog = tree.button_variant(format!("tog_{}", idx), if wf.enabled { "Désactiver" } else { "Activer" }, ButtonVariant::Secondary, true, leaf(108.0, 28.0)).unwrap();
            let b_fld = tree.button_variant(format!("fld_{}", idx), format!("Dossier: {}", wf.folder), ButtonVariant::Ghost, true, leaf(160.0, 28.0)).unwrap();
            let b_open = tree.button_variant(format!("open_{}", idx), "Studio >", ButtonVariant::Primary, true, leaf(96.0, 28.0)).unwrap();
            wf_rows.push(tree.container(&[info, b_tog, b_fld, b_open], row(8.0)).unwrap());
        }
        let mut p_s = col(8.0);
        p_s.size = Size { width: length(w), height: length(h - 70.0) };
        p_s.padding = Rect { left: length(14.0), right: length(14.0), top: length(12.0), bottom: length(12.0) };
        let panel = tree.panel(&wf_rows, None, None, p_s).unwrap();
        tree.container(&[kpis, panel], col(12.0)).unwrap()
    }

    fn build_studio(&self, tree: &mut WidgetTree, w: f32, h: f32) -> NodeId {
        let ws_name = self.workspace.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "workspace".to_string());
        let b_ws = tree.button_variant("btn_open_ws", format!("WS: {}", ws_name), ButtonVariant::Secondary, true, leaf(112.0, 26.0)).unwrap();
        let b_lw = tree.button_variant("left_wf", "Flux", if self.left_pane == LeftPane::Workflows { ButtonVariant::Primary } else { ButtonVariant::Ghost }, true, leaf(54.0, 26.0)).unwrap();
        let b_lp = tree.button_variant("left_pal", "Actions", if self.left_pane == LeftPane::Palette { ButtonVariant::Primary } else { ButtonVariant::Ghost }, true, leaf(68.0, 26.0)).unwrap();
        let top_row = tree.container(&[b_ws, b_lw, b_lp], row(4.0)).unwrap();
        let mut left_items = vec![top_row];

        match self.left_pane {
            LeftPane::Workflows => {
                for (idx, wf) in self.workflows.iter().enumerate() {
                    let st = if wf.enabled { "ON" } else { "OFF" };
                    let lbl = format!("[{}] {} ({})", wf.folder, wf.name, st);
                    let var = if idx == self.selected_idx { ButtonVariant::Primary } else { ButtonVariant::Ghost };
                    left_items.push(tree.button_variant(format!("wf_{}", idx), lbl, var, true, leaf(244.0, 30.0)).unwrap());
                }
            }
            LeftPane::Palette => {
                left_items.push(tree.label_muted("Cliquez une action (Fortra):", leaf(240.0, 20.0)).unwrap());
                for (idx, tpl) in ACTION_PALETTE.iter().enumerate() {
                    left_items.push(tree.button_variant(format!("pal_{}", idx), format!("[{}] {}", tpl.category, tpl.label), ButtonVariant::Secondary, true, leaf(244.0, 28.0)).unwrap());
                }
            }
        }
        let mut l_s = col(6.0);
        l_s.size = Size { width: length(268.0), height: length(h) };
        l_s.padding = Rect { left: length(10.0), right: length(10.0), top: length(10.0), bottom: length(10.0) };
        let left = tree.panel(&left_items, None, None, l_s).unwrap();

        let graph_w = (w - 268.0 - 400.0 - 24.0).max(300.0);
        let graph = tree.node_graph("wfl_graph", self.nodes.clone(), self.edges.clone(), self.pan, self.zoom, None, leaf(graph_w, h)).unwrap();

        let logs_preview = if self.last_logs.is_empty() {
            self.yaml_text.clone()
        } else {
            let mut out = String::from("# --- Dernier Run / Debug StepLogs ---\n");
            for l in &self.last_logs {
                out.push_str(&format!("# [{}] {} -> {}\n", l.status, l.step_id, l.output.as_ref().map(|v| v.to_string()).unwrap_or_default()));
            }
            out.push('\n');
            out.push_str(&self.yaml_text);
            out
        };
        let r_title = tree.label("YAML & Inspecteur StepLogs", leaf(370.0, 22.0)).unwrap();
        let r_ed = tree.code_editor("yaml_preview", logs_preview, "yaml", true, false, 0, None, leaf(376.0, (h - 46.0).max(200.0))).unwrap();
        let mut r_s = col(8.0);
        r_s.size = Size { width: length(400.0), height: length(h) };
        r_s.padding = Rect { left: length(10.0), right: length(10.0), top: length(10.0), bottom: length(10.0) };
        let right = tree.panel(&[r_title, r_ed], None, None, r_s).unwrap();

        tree.container(&[left, graph, right], row(12.0)).unwrap()
    }

    fn build_schedule(&self, tree: &mut WidgetTree, w: f32, h: f32) -> NodeId {
        let mut rows = vec![tree.label("Tâches Programmées (Triggers Cron)", leaf(500.0, 24.0)).unwrap()];
        for (idx, wf) in self.workflows.iter().enumerate() {
            let cron_txt = wf.cron_expr.as_deref().unwrap_or("Manuel uniquement");
            let lbl = tree.label(format!("{} [{}] • Cron: {}", wf.name, wf.status, cron_txt), leaf(440.0, 26.0)).unwrap();
            let b5 = tree.button_variant(format!("cron5_{}", idx), "*/5 * * * *", ButtonVariant::Secondary, true, leaf(110.0, 28.0)).unwrap();
            let b2h = tree.button_variant(format!("cron2h_{}", idx), "0 2 * * *", ButtonVariant::Secondary, true, leaf(100.0, 28.0)).unwrap();
            let boff = tree.button_variant(format!("cronoff_{}", idx), "Stop Cron", ButtonVariant::Ghost, true, leaf(96.0, 28.0)).unwrap();
            rows.push(tree.container(&[lbl, b5, b2h, boff], row(8.0)).unwrap());
        }
        let mut s = col(8.0);
        s.size = Size { width: length(w), height: length(h) };
        s.padding = Rect { left: length(14.0), right: length(14.0), top: length(12.0), bottom: length(12.0) };
        tree.panel(&rows, None, None, s).unwrap()
    }

    fn build_form_modal(&self, tree: &mut WidgetTree) -> NodeId {
        let tpl = &ACTION_PALETTE[self.form.template_idx];
        let hdr = tree.label(format!("Propriétés : {} (Tab champ, Entrée valider)", tpl.label), leaf(440.0, 24.0)).unwrap();
        let f_vals = [
            ("Step ID", &self.form.step_id),
            ("Nom", &self.form.step_name),
            ("Output var", &self.form.output_var),
            (tpl.f1_label, &self.form.field1),
            (tpl.f2_label, &self.form.field2),
            (tpl.f3_label, &self.form.field3),
        ];
        let mut items = vec![hdr];
        for (idx, (lbl, val)) in f_vals.iter().enumerate() {
            let marker = if self.form.active_field == idx { ">" } else { " " };
            let txt = format!("{} {}: {}", marker, lbl, val);
            items.push(tree.button_variant(format!("ff_{}", idx), txt, if self.form.active_field == idx { ButtonVariant::Primary } else { ButtonVariant::Secondary }, true, leaf(440.0, 30.0)).unwrap());
        }
        let b_ok = tree.button_variant("form_submit", "Ajouter au Workflow", ButtonVariant::Primary, true, leaf(210.0, 34.0)).unwrap();
        let b_can = tree.button_variant("form_cancel", "Annuler (Esc)", ButtonVariant::Ghost, true, leaf(140.0, 34.0)).unwrap();
        items.push(tree.container(&[b_ok, b_can], row(12.0)).unwrap());
        let mut ms = col(8.0);
        ms.size = Size { width: length(472.0), height: length(310.0) };
        ms.padding = Rect { left: length(16.0), right: length(16.0), top: length(14.0), bottom: length(14.0) };
        tree.panel(&items, Some([0.04, 0.06, 0.12, 0.98]), Some([0.38, 0.40, 0.94, 0.9]), ms).unwrap()
    }
}

impl ApplicationHandler for StudioApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() { return; }
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
                if let Some(r) = self.renderer.as_mut() { r.resize(size); }
                if let Some(w) = self.window.as_ref() { w.request_redraw(); }
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor_pos = (position.x as f32, position.y as f32);
                if let Some(w) = self.window.as_ref() { w.request_redraw(); }
            }
            WindowEvent::MouseInput { state: ElementState::Pressed, button: MouseButton::Left, .. } => {
                self.pressed = self.root.and_then(|r| self.tree.interaction_key_at(r, self.cursor_pos).ok().flatten());
                self.handle_click(self.cursor_pos.0, self.cursor_pos.1);
                if let Some(w) = self.window.as_ref() { w.request_redraw(); }
            }
            WindowEvent::MouseInput { state: ElementState::Released, button: MouseButton::Left, .. } => {
                self.pressed = None;
                if let Some(w) = self.window.as_ref() { w.request_redraw(); }
            }
            WindowEvent::KeyboardInput { event: KeyEvent { logical_key, state: ElementState::Pressed, .. }, .. } if self.form.open => {
                match logical_key {
                    Key::Named(NamedKey::Escape) => self.form.open = false,
                    Key::Named(NamedKey::Tab) => self.form.active_field = (self.form.active_field + 1) % 6,
                    Key::Named(NamedKey::Enter) => self.submit_form_step(),
                    Key::Named(NamedKey::Backspace) => { self.form.active_text_mut().pop(); }
                    Key::Character(s) => self.form.active_text_mut().push_str(s.as_str()),
                    _ => {}
                }
                if let Some(w) = self.window.as_ref() { w.request_redraw(); }
            }
            WindowEvent::RedrawRequested => self.render_frame(),
            _ => {}
        }
    }
}

fn pick_folder_dialog(initial: &std::path::Path) -> Option<PathBuf> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        use std::process::Command;
        let init_esc = initial.display().to_string().replace('\'', "''");
        let ps = format!(
            "Add-Type -AssemblyName System.Windows.Forms; $d = New-Object System.Windows.Forms.FolderBrowserDialog; $d.SelectedPath = '{}'; if ($d.ShowDialog() -eq [System.Windows.Forms.DialogResult]::OK) {{ Write-Output $d.SelectedPath }}",
            init_esc
        );
        let out = Command::new("powershell").args(["-NoProfile", "-Sta", "-Command", &ps]).creation_flags(0x0800_0000).output().ok()?;
        let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if s.is_empty() { None } else { Some(PathBuf::from(s)) }
    }
    #[cfg(not(windows))]
    { let _ = initial; None }
}

fn pick_yaml_file_dialog(initial: &std::path::Path) -> Option<PathBuf> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        use std::process::Command;
        let init_esc = initial.display().to_string().replace('\'', "''");
        let ps = format!(
            "Add-Type -AssemblyName System.Windows.Forms; $d = New-Object System.Windows.Forms.OpenFileDialog; $d.InitialDirectory = '{}'; $d.Filter = 'WFL YAML (*.yaml;*.yml)|*.yaml;*.yml'; if ($d.ShowDialog() -eq [System.Windows.Forms.DialogResult]::OK) {{ Write-Output $d.FileName }}",
            init_esc
        );
        let out = Command::new("powershell").args(["-NoProfile", "-Sta", "-Command", &ps]).creation_flags(0x0800_0000).output().ok()?;
        let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if s.is_empty() { None } else { Some(PathBuf::from(s)) }
    }
    #[cfg(not(windows))]
    { let _ = initial; None }
}

fn discover_workspace(cwd: PathBuf) -> PathBuf {
    for anc in cwd.ancestors().take(6) {
        if anc.join(".wf").join("wf.db").exists() {
            return anc.to_path_buf();
        }
    }
    cwd
}

fn main() {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let mut workspace: Option<PathBuf> = None;
    let mut db_path: Option<PathBuf> = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--workspace" => workspace = args.next().map(PathBuf::from),
            "--db" => db_path = args.next().map(PathBuf::from),
            _ => {}
        }
    }
    let workspace = workspace.unwrap_or_else(|| discover_workspace(cwd));
    let db = db_path.unwrap_or_else(|| workspace.join(".wf").join("wf.db"));
    let store = WfStore::open(&db).expect("Impossible d'ouvrir WfStore");
    let event_loop = EventLoop::new().expect("EventLoop error");
    let mut app = StudioApp::new(workspace, store);
    let _ = event_loop.run_app(&mut app);
}
