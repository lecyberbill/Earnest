use std::collections::HashMap;
use serde_json::Value;
use wf_core::ast::{
    ActionSpec, FlowSetParams, FlowSleepParams, LogLevel, StepSpec, SysExecParams, SysHttpParams,
    SysLogParams, VfsListParams, VfsLocation, VfsReadParams, VfsTransferParams, VfsWriteParams,
};

#[derive(Clone, Debug)]
pub struct ActionTemplate {
    pub action: &'static str,
    pub category: &'static str,
    pub label: &'static str,
    pub f1_label: &'static str,
    pub f1_default: &'static str,
    pub f2_label: &'static str,
    pub f2_default: &'static str,
    pub f3_label: &'static str,
    pub f3_default: &'static str,
}

pub const ACTION_PALETTE: &[ActionTemplate] = &[
    ActionTemplate {
        action: "vfs.list",
        category: "VFS",
        label: "vfs.list (Lister)",
        f1_label: "Connexion",
        f1_default: "local",
        f2_label: "Chemin dossier",
        f2_default: "in",
        f3_label: "Glob (*.txt)",
        f3_default: "*.txt",
    },
    ActionTemplate {
        action: "vfs.read",
        category: "VFS",
        label: "vfs.read (Lire)",
        f1_label: "Connexion",
        f1_default: "local",
        f2_label: "Chemin fichier",
        f2_default: "in/doc.txt",
        f3_label: "Encodage (utf8)",
        f3_default: "utf8",
    },
    ActionTemplate {
        action: "vfs.write",
        category: "VFS",
        label: "vfs.write (Écrire)",
        f1_label: "Connexion",
        f1_default: "local",
        f2_label: "Chemin fichier",
        f2_default: "out/result.txt",
        f3_label: "Contenu",
        f3_default: "Hello WFL",
    },
    ActionTemplate {
        action: "vfs.transfer",
        category: "VFS",
        label: "vfs.transfer (Copier)",
        f1_label: "Source (conn:path)",
        f1_default: "local:in/doc.txt",
        f2_label: "Dest (conn:path)",
        f2_default: "local:out/doc.txt",
        f3_label: "Overwrite (true/false)",
        f3_default: "true",
    },
    ActionTemplate {
        action: "vfs.delete",
        category: "VFS",
        label: "vfs.delete (Supprimer)",
        f1_label: "Connexion",
        f1_default: "local",
        f2_label: "Chemin fichier",
        f2_default: "out/temp.txt",
        f3_label: "(Inutilisé)",
        f3_default: "",
    },
    ActionTemplate {
        action: "flow.if",
        category: "Flow",
        label: "flow.if (Condition)",
        f1_label: "Condition",
        f1_default: "true",
        f2_label: "Message Then (sys.log)",
        f2_default: "Condition vraie",
        f3_label: "(Optionnel)",
        f3_default: "",
    },
    ActionTemplate {
        action: "flow.set",
        category: "Flow",
        label: "flow.set (Variable)",
        f1_label: "Nom variable",
        f1_default: "my_var",
        f2_label: "Valeur",
        f2_default: "valeur",
        f3_label: "(Inutilisé)",
        f3_default: "",
    },
    ActionTemplate {
        action: "flow.sleep",
        category: "Flow",
        label: "flow.sleep (Pause ms)",
        f1_label: "Durée (ms)",
        f1_default: "500",
        f2_label: "(Inutilisé)",
        f2_default: "",
        f3_label: "(Inutilisé)",
        f3_default: "",
    },
    ActionTemplate {
        action: "sys.exec",
        category: "System",
        label: "sys.exec (Commande)",
        f1_label: "Programme",
        f1_default: "cmd",
        f2_label: "Arguments (espaces)",
        f2_default: "/C echo ok",
        f3_label: "CWD (optionnel)",
        f3_default: "",
    },
    ActionTemplate {
        action: "sys.http",
        category: "System",
        label: "sys.http (Requête API)",
        f1_label: "Méthode (GET/POST)",
        f1_default: "GET",
        f2_label: "URL complète",
        f2_default: "https://httpbin.org/get",
        f3_label: "Connexion (opt.)",
        f3_default: "",
    },
    ActionTemplate {
        action: "sys.log",
        category: "System",
        label: "sys.log (Journal)",
        f1_label: "Niveau (info/warn/error)",
        f1_default: "info",
        f2_label: "Message",
        f2_default: "Étape terminée",
        f3_label: "(Inutilisé)",
        f3_default: "",
    },
];

#[derive(Clone, Debug, Default)]
pub struct FormModalState {
    pub open: bool,
    pub template_idx: usize,
    pub step_id: String,
    pub step_name: String,
    pub output_var: String,
    pub field1: String,
    pub field2: String,
    pub field3: String,
    pub active_field: usize,
}

impl FormModalState {
    pub fn open_for(&mut self, template_idx: usize, next_num: usize) {
        let tpl = &ACTION_PALETTE[template_idx.min(ACTION_PALETTE.len() - 1)];
        self.open = true;
        self.template_idx = template_idx;
        self.step_id = format!("step_{}", next_num);
        self.step_name = tpl.label.to_string();
        self.output_var = String::new();
        self.field1 = tpl.f1_default.to_string();
        self.field2 = tpl.f2_default.to_string();
        self.field3 = tpl.f3_default.to_string();
        self.active_field = 0;
    }

    pub fn active_text_mut(&mut self) -> &mut String {
        match self.active_field % 6 {
            0 => &mut self.step_id,
            1 => &mut self.step_name,
            2 => &mut self.output_var,
            3 => &mut self.field1,
            4 => &mut self.field2,
            _ => &mut self.field3,
        }
    }

    pub fn build_step(&self) -> Result<StepSpec, String> {
        let id = self.step_id.trim();
        if id.is_empty() {
            return Err("L'ID d'étape est requis".to_string());
        }
        let tpl = &ACTION_PALETTE[self.template_idx];
        let action = match tpl.action {
            "vfs.list" => ActionSpec::VfsList {
                params: VfsListParams {
                    conn: self.field1.trim().to_string(),
                    path: self.field2.trim().to_string(),
                    glob: Some(self.field3.trim().to_string()).filter(|s| !s.is_empty()),
                    recursive: false,
                },
            },
            "vfs.read" => ActionSpec::VfsRead {
                params: VfsReadParams {
                    conn: self.field1.trim().to_string(),
                    path: self.field2.trim().to_string(),
                    encoding: Default::default(),
                },
            },
            "vfs.write" => ActionSpec::VfsWrite {
                params: VfsWriteParams {
                    conn: self.field1.trim().to_string(),
                    path: self.field2.trim().to_string(),
                    content: self.field3.clone(),
                    create_dirs: true,
                },
            },
            "vfs.transfer" => {
                let (sc, sp) = self.field1.split_once(':').unwrap_or(("local", self.field1.trim()));
                let (dc, dp) = self.field2.split_once(':').unwrap_or(("local", self.field2.trim()));
                ActionSpec::VfsTransfer {
                    params: VfsTransferParams {
                        src: VfsLocation { conn: sc.trim().to_string(), path: sp.trim().to_string() },
                        dst: VfsLocation { conn: dc.trim().to_string(), path: dp.trim().to_string() },
                        overwrite: self.field3.trim() != "false",
                    },
                }
            }
            "vfs.delete" => ActionSpec::VfsDelete {
                params: VfsLocation {
                    conn: self.field1.trim().to_string(),
                    path: self.field2.trim().to_string(),
                },
            },
            "flow.if" => ActionSpec::FlowIf {
                condition: self.field1.trim().to_string(),
                then: vec![StepSpec {
                    id: format!("{}_then", id),
                    name: Some("Then log".to_string()),
                    action: ActionSpec::SysLog {
                        params: SysLogParams {
                            level: LogLevel::Info,
                            message: self.field2.clone(),
                        },
                    },
                    output: None,
                    timeout_ms: None,
                    retry: None,
                    on_error: vec![],
                }],
                else_branch: vec![],
            },
            "flow.set" => ActionSpec::FlowSet {
                params: FlowSetParams {
                    var: self.field1.trim().to_string(),
                    value: Value::String(self.field2.clone()),
                },
            },
            "flow.sleep" => ActionSpec::FlowSleep {
                params: FlowSleepParams {
                    duration_ms: self.field1.trim().parse::<u64>().unwrap_or(500),
                },
            },
            "sys.exec" => ActionSpec::SysExec {
                params: SysExecParams {
                    program: self.field1.trim().to_string(),
                    args: self.field2.split_whitespace().map(String::from).collect(),
                    cwd: Some(self.field3.trim().to_string()).filter(|s| !s.is_empty()),
                    env: HashMap::new(),
                    allow_non_zero: false,
                },
            },
            "sys.http" => ActionSpec::SysHttp {
                params: SysHttpParams {
                    conn: Some(self.field3.trim().to_string()).filter(|s| !s.is_empty()),
                    method: self.field1.trim().to_uppercase(),
                    url: self.field2.trim().to_string(),
                    headers: HashMap::new(),
                    query: HashMap::new(),
                    body: None,
                    parse_json: true,
                },
            },
            _ => ActionSpec::SysLog {
                params: SysLogParams {
                    level: match self.field1.trim().to_lowercase().as_str() {
                        "warn" => LogLevel::Warn,
                        "error" => LogLevel::Error,
                        "debug" => LogLevel::Debug,
                        _ => LogLevel::Info,
                    },
                    message: self.field2.clone(),
                },
            },
        };

        Ok(StepSpec {
            id: id.to_string(),
            name: Some(self.step_name.trim().to_string()).filter(|s| !s.is_empty()),
            action,
            output: Some(self.output_var.trim().to_string()).filter(|s| !s.is_empty()),
            timeout_ms: None,
            retry: None,
            on_error: vec![],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_valid_step_from_form_modal() {
        let mut form = FormModalState::default();
        form.open_for(3, 1); // vfs.transfer
        form.step_id = "copy_a".to_string();
        let step = form.build_step().unwrap();
        assert_eq!(step.id, "copy_a");
        assert!(matches!(step.action, ActionSpec::VfsTransfer { .. }));
    }
}
