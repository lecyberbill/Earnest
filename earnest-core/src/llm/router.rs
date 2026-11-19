use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use super::client::{ChatMessage, ChatResponse, LLMClient};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackendConfig {
    pub id: String,
    pub name: String,
    pub provider_type: String,
    pub base_url: String,
    pub model: String,
    pub api_key: Option<String>,
    pub is_enabled: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ProbeResult {
    pub id: String,
    pub name: String,
    pub provider_type: String,
    pub model: String,
    pub is_online: bool,
    pub latency_ms: Option<u128>,
    pub error: Option<String>,
    pub available_models: Vec<String>,
}

pub struct LLMRouter {
    pub backends: Vec<BackendConfig>,
}

impl LLMRouter {
    pub fn from_db(conn: &Connection) -> Self {
        let mut backends = Vec::new();
        if let Ok(mut stmt) = conn.prepare("SELECT id, name, provider_type, base_url, model, api_key, is_enabled FROM model_providers ORDER BY priority ASC, created_at ASC") {
            if let Ok(rows) = stmt.query_map([], |row| {
                Ok(BackendConfig {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    provider_type: row.get(2)?,
                    base_url: row.get(3)?,
                    model: row.get(4)?,
                    api_key: row.get(5)?,
                    is_enabled: row.get::<_, i64>(6)? != 0,
                })
            }) {
                for r in rows.flatten() {
                    if r.is_enabled {
                        backends.push(r);
                    }
                }
            }
        }

        if backends.is_empty() {
            // Fallbacks
            backends.push(BackendConfig {
                id: "ollama".to_string(),
                name: "Ollama Local".to_string(),
                provider_type: "ollama".to_string(),
                base_url: "http://192.168.1.165:11434".to_string(),
                model: "qwen3.8:27b".to_string(),
                api_key: None,
                is_enabled: true,
            });
            backends.push(BackendConfig {
                id: "lmstudio".to_string(),
                name: "LM Studio".to_string(),
                provider_type: "openai".to_string(),
                base_url: "http://127.0.0.1:1234".to_string(),
                model: "local-model".to_string(),
                api_key: None,
                is_enabled: true,
            });
        }

        Self { backends }
    }

    pub fn probe_all(&self) -> Vec<ProbeResult> {
        let http = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(3))
            .build()
            .unwrap_or_else(|_| reqwest::blocking::Client::new());

        self.backends.iter().map(|b| {
            let client = LLMClient::new(&b.provider_type, &b.base_url, &b.model, b.api_key.clone());
            match client.ping() {
                Ok(ms) => {
                    let available_models = fetch_models_list(&http, b);
                    ProbeResult {
                        id: b.id.clone(),
                        name: b.name.clone(),
                        provider_type: b.provider_type.clone(),
                        model: b.model.clone(),
                        is_online: true,
                        latency_ms: Some(ms),
                        error: None,
                        available_models,
                    }
                }
                Err(err) => ProbeResult {
                    id: b.id.clone(),
                    name: b.name.clone(),
                    provider_type: b.provider_type.clone(),
                    model: b.model.clone(),
                    is_online: false,
                    latency_ms: None,
                    error: Some(err),
                    available_models: Vec::new(),
                },
            }
        }).collect()
    }

    /// Estime la limite de caractères sûre avant compaction pour un backend donné.
    pub fn estimate_context_chars(&self, backend_id: &str) -> usize {
        let Some(b) = self.backends.iter().find(|x| x.id == backend_id) else {
            return 60_000;
        };
        if b.base_url.contains("deepseek") {
            return 160_000;
        }
        if b.provider_type == "ollama" {
            let http = reqwest::blocking::Client::builder()
                .timeout(std::time::Duration::from_secs(2))
                .build()
                .unwrap_or_else(|_| reqwest::blocking::Client::new());
            let url = format!("{}/api/show", b.base_url.trim_end_matches('/'));
            if let Ok(resp) = http.post(&url).json(&serde_json::json!({"model": b.model})).send() {
                if let Ok(val) = resp.json::<serde_json::Value>() {
                    if let Some(params) = val["parameters"].as_str() {
                        for line in params.lines() {
                            if line.trim_start().starts_with("num_ctx") {
                                if let Some(n) = line.split_whitespace().last().and_then(|s| s.parse::<usize>().ok()) {
                                    return (n * 3).clamp(16_000, 180_000);
                                }
                            }
                        }
                    }
                }
            }
        }
        // LM Studio / OpenAI local : seuil conservateur par défaut (évite de saturer 8k-16k)
        if b.base_url.contains("127.0.0.1") || b.base_url.contains("localhost") {
            return 24_000;
        }
        60_000
    }

    pub fn chat_completion_with_fallback(
        &self,
        messages: &[ChatMessage],
        tools: Option<&[serde_json::Value]>,
        think_effort: &str,
    ) -> Result<(ChatResponse, String), String> {
        let mut last_err = String::from("Aucun moteur configuré");

        for b in &self.backends {
            if crate::llm::client::is_cancelled() {
                return Err("Run annulé par l'utilisateur.".to_string());
            }
            let client = LLMClient::new(&b.provider_type, &b.base_url, &b.model, b.api_key.clone());
            match client.chat_completion(messages, tools, think_effort) {
                Ok(resp) => return Ok((resp, b.id.clone())),
                Err(e) => {
                    if crate::llm::client::is_cancelled() || e.to_lowercase().contains("annulé") {
                        return Err(e);
                    }
                    last_err = format!("Moteur {} échoué: {}", b.name, e);
                }
            }
        }

        Err(format!("Tous les moteurs LLM ont échoué. Dernière erreur: {}", last_err))
    }
}

fn fetch_models_list(http: &reqwest::blocking::Client, b: &BackendConfig) -> Vec<String> {
    let base = b.base_url.trim_end_matches('/');
    if b.provider_type == "ollama" {
        if let Ok(resp) = http.get(format!("{}/api/tags", base)).send() {
            if let Ok(val) = resp.json::<serde_json::Value>() {
                if let Some(arr) = val["models"].as_array() {
                    return arr.iter().filter_map(|m| m["name"].as_str().map(|s| s.to_string())).collect();
                }
            }
        }
    } else {
        let mut req = http.get(format!("{}/v1/models", base));
        if let Some(ref key) = b.api_key {
            req = req.header("Authorization", format!("Bearer {}", key));
        }
        if let Ok(resp) = req.send() {
            if let Ok(val) = resp.json::<serde_json::Value>() {
                if let Some(arr) = val["data"].as_array() {
                    return arr.iter().filter_map(|m| m["id"].as_str().map(|s| s.to_string())).collect();
                }
            }
        }
    }
    Vec::new()
}

