use std::collections::BTreeMap;
use std::io::{BufRead, BufReader};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub static CANCEL_RUN: AtomicBool = AtomicBool::new(false);

pub fn request_cancel() {
    CANCEL_RUN.store(true, Ordering::SeqCst);
}

pub fn reset_cancel() {
    CANCEL_RUN.store(false, Ordering::SeqCst);
}

pub fn is_cancelled() -> bool {
    CANCEL_RUN.load(Ordering::SeqCst)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub images: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatResponse {
    pub content: String,
    pub tool_calls: Vec<ToolCallItem>,
    pub prompt_tokens: usize,
    pub completion_tokens: usize,
    pub total_tokens: usize,
    pub reasoning_content: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallItem {
    pub id: String,
    pub name: String,
    pub arguments: Value,
}

pub struct LLMClient {
    client: Client,
    pub provider_type: String,
    pub base_url: String,
    pub model: String,
    pub api_key: Option<String>,
}

impl LLMClient {
    pub fn new(provider_type: &str, base_url: &str, model: &str, api_key: Option<String>) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(300))
            .build()
            .unwrap_or_else(|_| Client::new());

        Self {
            client,
            provider_type: provider_type.to_string(),
            base_url: base_url.trim_end_matches('/').to_string(),
            model: model.to_string(),
            api_key,
        }
    }

    pub fn ping(&self) -> Result<u128, String> {
        let start = std::time::Instant::now();
        let ping_url = if self.provider_type == "ollama" {
            format!("{}/api/tags", self.base_url)
        } else {
            format!("{}/v1/models", self.base_url)
        };

        let mut req = self.client.get(&ping_url);
        if let Some(key) = &self.api_key {
            req = req.header("Authorization", format!("Bearer {}", key));
        }

        match req.send() {
            Ok(res) if res.status().is_success() => Ok(start.elapsed().as_millis()),
            Ok(res) => Err(format!("Statut HTTP non valide : {}", res.status())),
            Err(e) => Err(e.to_string()),
        }
    }

    pub fn chat_completion(
        &self,
        messages: &[ChatMessage],
        tools: Option<&[Value]>,
        think_effort: &str,
    ) -> Result<ChatResponse, String> {
        if is_cancelled() {
            return Err("Run annulé par l'utilisateur.".to_string());
        }
        if self.provider_type == "ollama" {
            self.chat_ollama(messages, tools, think_effort)
        } else {
            self.chat_openai(messages, tools, think_effort)
        }
    }

    fn chat_ollama(
        &self,
        messages: &[ChatMessage],
        tools: Option<&[Value]>,
        think_effort: &str,
    ) -> Result<ChatResponse, String> {
        let url = format!("{}/api/chat", self.base_url);
        let enable_think = !matches!(think_effort.to_lowercase().as_str(), "off" | "none" | "false" | "");
        let mut body = serde_json::json!({
            "model": self.model,
            "messages": messages,
            "stream": true,
            "think": enable_think
        });

        if let Some(t) = tools {
            if !t.is_empty() {
                body["tools"] = serde_json::json!(t);
            }
        }

        let resp = self.client.post(&url)
            .json(&body)
            .send()
            .map_err(|e| format!("Erreur réseau Ollama: {}", e))?;

        if !resp.status().is_success() {
            return Err(format!("Erreur Ollama statut {}", resp.status()));
        }

        let mut content = String::new();
        let mut reasoning = String::new();
        let mut tool_calls = Vec::new();
        let mut prompt_tokens = 0usize;
        let mut completion_tokens = 0usize;

        let reader = BufReader::new(resp);
        for line_res in reader.lines() {
            if is_cancelled() {
                return Err("Run annulé par l'utilisateur.".to_string());
            }
            let line = match line_res {
                Ok(l) => l,
                Err(e) => return Err(format!("Erreur lecture stream Ollama: {}", e)),
            };
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let data: Value = match serde_json::from_str(trimmed) {
                Ok(v) => v,
                Err(_) => continue,
            };
            let msg = &data["message"];
            if let Some(think_chunk) = msg["thinking"].as_str() {
                reasoning.push_str(think_chunk);
            }
            if let Some(chunk) = msg["content"].as_str() {
                content.push_str(chunk);
            }
            if let Some(arr) = msg["tool_calls"].as_array() {
                for call in arr {
                    let func = &call["function"];
                    let name = func["name"].as_str().unwrap_or("").to_string();
                    let raw = &func["arguments"];
                    let args = if let Some(s) = raw.as_str() {
                        parse_tool_args_with_repair(&name, s)
                    } else {
                        raw.clone()
                    };
                    let call_id = format!("call_{}", &uuid::Uuid::new_v4().to_string().replace('-', "")[..8]);
                    tool_calls.push(ToolCallItem { id: call_id, name, arguments: args });
                }
            }
            if data["done"].as_bool().unwrap_or(false) {
                prompt_tokens = data["prompt_eval_count"].as_u64().unwrap_or(0) as usize;
                completion_tokens = data["eval_count"].as_u64().unwrap_or(0) as usize;
            }
        }

        Ok(ChatResponse {
            content,
            tool_calls,
            prompt_tokens,
            completion_tokens,
            total_tokens: prompt_tokens + completion_tokens,
            reasoning_content: if reasoning.is_empty() { None } else { Some(reasoning) },
        })
    }

    fn chat_openai(
        &self,
        messages: &[ChatMessage],
        tools: Option<&[Value]>,
        think_effort: &str,
    ) -> Result<ChatResponse, String> {
        let url = format!("{}/v1/chat/completions", self.base_url);
        let enable_think = !matches!(think_effort.to_lowercase().as_str(), "off" | "none" | "false" | "");
        let mut body = serde_json::json!({
            "model": self.model,
            "messages": messages,
            "temperature": 0.2,
            "stream": true
        });
        if self.base_url.contains("deepseek") {
            body["thinking"] = serde_json::json!({
                "type": if enable_think { "enabled" } else { "disabled" }
            });
        }

        if let Some(t) = tools {
            if !t.is_empty() {
                body["tools"] = serde_json::json!(t);
            }
        }

        let mut req = self.client.post(&url).json(&body);
        if let Some(key) = &self.api_key {
            req = req.header("Authorization", format!("Bearer {}", key));
        }

        let resp = req.send().map_err(|e| format!("Erreur réseau OpenAI: {}", e))?;
        if !resp.status().is_success() {
            return Err(format!("Erreur OpenAI statut {}", resp.status()));
        }

        let mut content = String::new();
        let mut reasoning = String::new();
        let mut tc_acc: BTreeMap<usize, (String, String, String)> = BTreeMap::new();
        let mut prompt_tokens = 0usize;
        let mut completion_tokens = 0usize;

        let reader = BufReader::new(resp);
        for line_res in reader.lines() {
            if is_cancelled() {
                return Err("Run annulé par l'utilisateur.".to_string());
            }
            let line = match line_res {
                Ok(l) => l,
                Err(e) => return Err(format!("Erreur lecture stream OpenAI: {}", e)),
            };
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with(':') {
                continue;
            }
            let payload_str = if let Some(s) = trimmed.strip_prefix("data: ") {
                s.trim()
            } else if let Some(s) = trimmed.strip_prefix("data:") {
                s.trim()
            } else {
                continue;
            };
            if payload_str == "[DONE]" {
                break;
            }
            let data: Value = match serde_json::from_str(payload_str) {
                Ok(v) => v,
                Err(_) => continue,
            };
            if let Some(usage) = data.get("usage").filter(|u| !u.is_null()) {
                prompt_tokens = usage["prompt_tokens"].as_u64().unwrap_or(prompt_tokens as u64) as usize;
                completion_tokens = usage["completion_tokens"].as_u64().unwrap_or(completion_tokens as u64) as usize;
            }
            let delta = &data["choices"][0]["delta"];
            if let Some(rc) = delta["reasoning_content"].as_str() {
                reasoning.push_str(rc);
            }
            if let Some(c) = delta["content"].as_str() {
                content.push_str(c);
            }
            if let Some(tcs) = delta["tool_calls"].as_array() {
                for tc in tcs {
                    let idx = tc["index"].as_u64().unwrap_or(0) as usize;
                    let entry = tc_acc.entry(idx).or_insert_with(|| (String::new(), String::new(), String::new()));
                    if let Some(id) = tc["id"].as_str() {
                        entry.0 = id.to_string();
                    }
                    if let Some(name) = tc["function"]["name"].as_str() {
                        entry.1.push_str(name);
                    }
                    if let Some(args_part) = tc["function"]["arguments"].as_str() {
                        entry.2.push_str(args_part);
                    }
                }
            }
        }

        let mut tool_calls = Vec::new();
        for (_idx, (call_id, name, raw_args)) in tc_acc {
            let parsed_args = parse_tool_args_with_repair(&name, &raw_args);
            let final_id = if call_id.is_empty() { "call_gen".to_string() } else { call_id };
            tool_calls.push(ToolCallItem { id: final_id, name, arguments: parsed_args });
        }

        Ok(ChatResponse {
            content,
            tool_calls,
            prompt_tokens,
            completion_tokens,
            total_tokens: prompt_tokens + completion_tokens,
            reasoning_content: if reasoning.is_empty() { None } else { Some(reasoning) },
        })
    }
}

/// Parse les arguments JSON d'un appel d'outil avec réparation chirurgicale via `json-repair`
/// si `serde_json` échoue, évitant la perte silencieuse des arguments (`{}`).
pub fn parse_tool_args_with_repair(name: &str, raw_args: &str) -> Value {
    let trimmed = raw_args.trim();
    if trimmed.is_empty() {
        return serde_json::json!({});
    }
    if let Ok(v) = serde_json::from_str::<Value>(trimmed) {
        return v;
    }
    let repaired = json_repair::repair(trimmed);
    if let Ok(v) = serde_json::from_str::<Value>(&repaired) {
        if v.is_object() {
            println!(
                "  🩹 [JSON-REPAIR] Arguments réparés pour '{}' ({} -> {} chars)",
                name,
                raw_args.len(),
                repaired.len()
            );
            return v;
        }
    }
    println!(
        "  ⚠️ [LLM] Tool '{}' arguments irréparables ({} chars), repli sur {{}}",
        name,
        raw_args.len()
    );
    serde_json::json!({})
}

#[cfg(test)]
mod tests {
    use super::parse_tool_args_with_repair;

    #[test]
    fn repairs_malformed_tool_args_instead_of_dropping_to_empty() {
        let raw = "```json\n{\"command\": \"cargo test\", \"cwd\": \"src\"";
        let v = parse_tool_args_with_repair("execute_terminal", raw);
        assert_eq!(v["command"], "cargo test");
        assert_eq!(v["cwd"], "src");
    }

    #[test]
    fn preserves_valid_tool_args() {
        let raw = r#"{"key": "status", "value": "ok"}"#;
        let v = parse_tool_args_with_repair("store_memory", raw);
        assert_eq!(v["key"], "status");
        assert_eq!(v["value"], "ok");
    }
}

