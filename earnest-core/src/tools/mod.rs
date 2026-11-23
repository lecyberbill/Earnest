pub mod terminal;
pub mod artifacts;
pub mod fs;
pub mod web;

use serde_json::Value;

pub fn resolve_kb_db_path() -> std::path::PathBuf {
    let mut candidates = vec![
        std::path::PathBuf::from("earnest-bcpe/kb.db"),
        std::path::PathBuf::from("../earnest-bcpe/kb.db"),
        std::path::PathBuf::from("../../earnest-bcpe/kb.db"),
        std::path::PathBuf::from("kb.db"),
    ];
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join("earnest-bcpe/kb.db"));
            candidates.push(dir.join("../../../earnest-bcpe/kb.db"));
            candidates.push(dir.join("../../../../earnest-bcpe/kb.db"));
        }
    }
    if let Some(home) = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")) {
        candidates.push(std::path::PathBuf::from(home).join(".local/share/earnest-bcpe/kb.db"));
    }
    candidates
        .into_iter()
        .find(|p| p.exists())
        .unwrap_or_else(|| std::path::PathBuf::from("earnest-bcpe/kb.db"))
}

pub fn get_primitive_tools_schemas() -> Vec<Value> {
    vec![
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "web_search",
                "description": "Recherche sur le web (DuckDuckGo) pour trouver des informations récentes, des cotations, des actualités ou des documentations en direct.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "query": { "type": "string", "description": "Mots-clés de recherche" },
                        "max_results": { "type": "integer", "description": "Nombre max de résultats (défaut: 5)" }
                    },
                    "required": ["query"]
                }
            }
        }),
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "web_fetch",
                "description": "Télécharge et extrait le texte d'une page web via son URL HTTP/HTTPS.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "url": { "type": "string", "description": "L'URL complète de la page web à lire" },
                        "max_chars": { "type": "integer", "description": "Nombre max de caractères textuels à conserver (défaut: 6000)" }
                    },
                    "required": ["url"]
                }
            }
        }),
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "terminal_exec",
                "description": "Exécute une commande terminal sécurisée (PowerShell / Bash) pour inspecter l'hôte ou manipuler des fichiers.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "command": { "type": "string", "description": "La commande exacte à exécuter" },
                        "timeout_sec": { "type": "integer", "description": "Délai limite en secondes (défaut: 30)" }
                    },
                    "required": ["command"]
                }
            }
        }),
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "store_memory",
                "description": "Persiste une information clé dans la mémoire à long terme (scope 'project' ou 'session').",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "key": { "type": "string", "description": "Clé d'identification de la mémoire" },
                        "content": { "type": "string", "description": "Contenu textuel ou Markdown à mémoriser" },
                        "scope": { "type": "string", "enum": ["project", "session"], "description": "Portée de rétention" }
                    },
                    "required": ["key", "content"]
                }
            }
        }),
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "recall_memory",
                "description": "Recherche et extrait une information précédemment mémorisée par sa clé.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "key": { "type": "string", "description": "Clé de la mémoire à retrouver" }
                    },
                    "required": ["key"]
                }
            }
        }),
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "read_file",
                "description": "Lit le contenu numéroté d'un fichier texte sur disque (avec plage de lignes optionnelle start_line..end_line, max 400 lignes par appel).",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "Chemin du fichier à lire" },
                        "start_line": { "type": "integer", "description": "Première ligne (1-based, optionnel)" },
                        "end_line": { "type": "integer", "description": "Dernière ligne (1-based inclus, optionnel)" }
                    },
                    "required": ["path"]
                }
            }
        }),
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "edit_file",
                "description": "Modifie précisément un fichier existant par patch atomique (remplace un bloc `target` unique par `replacement`).",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "Chemin du fichier à modifier" },
                        "target": { "type": "string", "description": "Bloc de texte exact existant à remplacer" },
                        "replacement": { "type": "string", "description": "Nouveau texte de remplacement" }
                    },
                    "required": ["path", "target", "replacement"]
                }
            }
        }),
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "list_dir",
                "description": "Liste les fichiers et sous-dossiers d'un répertoire (hors .git, target, node_modules).",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "Répertoire à lister (défaut: '.')" },
                        "max_entries": { "type": "integer", "description": "Nombre max d'entrées (défaut: 80)" }
                    }
                }
            }
        }),
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "grep_search",
                "description": "Recherche un motif texte dans les fichiers d'un répertoire ou fichier et retourne fichier, numéro de ligne et extrait.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "query": { "type": "string", "description": "Texte recherché" },
                        "path": { "type": "string", "description": "Répertoire ou fichier cible (défaut: '.')" },
                        "max_results": { "type": "integer", "description": "Nombre max de résultats (défaut: 30)" },
                        "case_sensitive": { "type": "boolean", "description": "Sensible à la casse (défaut: false)" }
                    },
                    "required": ["query"]
                }
            }
        }),
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "symbol_search",
                "description": "Indexe et recherche rapidement des symboles (fn, struct, enum, class, export, interface) dans le workspace avec lecture ciblée par plage de lignes (économise les tokens).",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "query": { "type": "string", "description": "Nom ou motif flou du symbole recherché" },
                        "path": { "type": "string", "description": "Répertoire racine optionnel (défaut: '.')" },
                        "limit": { "type": "integer", "description": "Nombre max de résultats (défaut: 8)" },
                        "include_snippet": { "type": "boolean", "description": "Inclure un extrait ciblé autour de la ligne (défaut: true)" }
                    },
                    "required": ["query"]
                }
            }
        }),
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "workspace_map",
                "description": "Génère une carte compacte et priorisée du workspace (types avant fonctions, 1 ligne par fichier) optimisée pour les prompts LLM.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "Répertoire racine optionnel (défaut: '.')" },
                        "max_files": { "type": "integer", "description": "Nombre max de fichiers listés (défaut: 25)" },
                        "max_symbols_per_file": { "type": "integer", "description": "Nombre max de symboles par fichier (défaut: 8)" }
                    }
                }
            }
        }),
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "create_artifact",
                "description": "Génère un livrable matérialisé (Markdown, Python, Rust, HTML, etc.) sauvegardé sur disque.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "title": { "type": "string", "description": "Titre du livrable" },
                        "content": { "type": "string", "description": "Contenu intégral du fichier" },
                        "artifact_type": { "type": "string", "description": "Type: markdown, rust, python, json, html" },
                        "file_path": { "type": "string", "description": "Chemin cible optionnel" }
                    },
                    "required": ["title", "content"]
                }
            }
        }),
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "kb_list_products",
                "description": "Liste tous les produits/technologies disponibles dans la base de connaissances d'expérience (BCPE) avant d'appeler kb_search.",
                "parameters": {
                    "type": "object",
                    "properties": {}
                }
            }
        }),
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "kb_search",
                "description": "Recherche dans la base de connaissances d'expérience (BCPE) les pièges, règles et solutions connus pour un produit (appelle kb_list_products si tu ne connais pas le product_id exact, ex: 'powershell', 'rust', 'earnest').",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "product_id": { "type": "string", "description": "Identifiant du produit ou technologie (ex: 'powershell', 'rust', 'earnest')" },
                        "query": { "type": "string", "description": "Mots-clés du problème rencontré, fonction ou symptôme" }
                    },
                    "required": ["product_id", "query"]
                }
            }
        }),
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "kb_propose",
                "description": "Enregistre une découverte, un bug résolu ou un piège évité dans la base de connaissances d'expérience (BCPE) pour qu'il ne se reproduise plus jamais.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "product_id": { "type": "string", "description": "Identifiant du produit (ex: 'powershell', 'rust', 'earnest')" },
                        "subject_key": { "type": "string", "description": "Clé slug unique du sujet (ex: 'powershell-pipe-syntax')" },
                        "kind": { "type": "string", "description": "Type d'entrée: 'piege', 'regle', 'catalogue'" },
                        "title": { "type": "string", "description": "Titre explicite du piège ou de la découverte" },
                        "symptom": { "type": "string", "description": "Message d'erreur exact ou symptôme constaté" },
                        "cause": { "type": "string", "description": "Cause racine technique identifiée" },
                        "correction": { "type": "string", "description": "Solution technique exacte appliquée pour corriger" },
                        "example": { "type": "string", "description": "Extrait de code minimal d'illustration (optionnel)" }
                    },
                    "required": ["product_id", "subject_key", "kind", "title", "correction"]
                }
            }
        })
    ]
}

