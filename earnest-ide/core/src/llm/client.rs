use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;

static OLLAMA_THINK_CAPS: OnceLock<Mutex<HashMap<String, Value>>> = OnceLock::new();
pub static CANCEL_LLM: AtomicBool = AtomicBool::new(false);

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

    fn resolve_ollama_think(&self, effort: &str) -> Value {
        if effort == "off" || effort == "none" || effort == "false" {
            return serde_json::json!(false);
        }
        let cache = OLLAMA_THINK_CAPS.get_or_init(|| Mutex::new(HashMap::new()));
        let supports_tiers = {
            if let Ok(guard) = cache.lock() {
                guard.get(&self.model).and_then(|v| v.as_bool())
            } else {
                None
            }
        };
        let has_tiers = match supports_tiers {
            Some(b) => b,
            None => {
                let show_url = format!("{}/api/show", self.base_url);
                let mut found = false;
                if let Ok(resp) = self.client.post(&show_url).json(&serde_json::json!({ "model": self.model })).send() {
                    if let Ok(data) = resp.json::<Value>() {
                        if let Some(values) = data["thinking"]["values"].as_array() {
                            found = values.iter().any(|v| v.as_str() == Some("low") || v.as_str() == Some("medium"));
                        }
                    }
                }
                if let Ok(mut guard) = cache.lock() {
                    guard.insert(self.model.clone(), serde_json::json!(found));
                }
                found
            }
        };
        if has_tiers {
            let tier = if effort == "on" || effort == "true" { "medium" } else { effort };
            serde_json::json!(tier)
        } else {
            serde_json::json!(true)
        }
    }

    pub fn chat_completion(
        &self,
        messages: &[ChatMessage],
        tools: Option<&[Value]>,
    ) -> Result<ChatResponse, String> {
        self.chat_completion_stream(messages, tools, "off", &mut |_, _| {})
    }

    pub fn chat_completion_stream(
        &self,
        messages: &[ChatMessage],
        tools: Option<&[Value]>,
        think_effort: &str,
        on_token: &mut dyn FnMut(&str, &str),
    ) -> Result<ChatResponse, String> {
        if self.provider_type == "ollama" {
            self.chat_ollama(messages, tools, think_effort, on_token)
        } else {
            self.chat_openai(messages, tools, think_effort, on_token)
        }
    }

    fn chat_ollama(
        &self,
        messages: &[ChatMessage],
        tools: Option<&[Value]>,
        think_effort: &str,
        on_token: &mut dyn FnMut(&str, &str),
    ) -> Result<ChatResponse, String> {
        let url = format!("{}/api/chat", self.base_url);
        let think_val = self.resolve_ollama_think(think_effort);
        let mut body = serde_json::json!({
            "model": self.model,
            "messages": messages,
            "stream": true,
            "think": think_val,
            "options": {
                "num_predict": 16384
            }
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

        let reader = BufReader::new(resp);
        let mut content = String::new();
        let mut tool_calls = Vec::new();
        let mut prompt_tokens = 0usize;
        let mut completion_tokens = 0usize;
        let mut done_reason = String::new();

        for line in reader.lines() {
            if CANCEL_LLM.load(Ordering::SeqCst) {
                return Err("⏹️ Stream annulé par l'utilisateur.".to_string());
            }
            let line = line.map_err(|e| format!("Erreur lecture stream Ollama: {}", e))?;
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
                if !think_chunk.is_empty() {
                    on_token("thinking", think_chunk);
                }
            }
            if let Some(chunk) = msg["content"].as_str() {
                if !chunk.is_empty() {
                    on_token("content", chunk);
                }
                content.push_str(chunk);
            }
            if let Some(arr) = msg["tool_calls"].as_array() {
                for call in arr {
                    let func = &call["function"];
                    let name = func["name"].as_str().unwrap_or("").to_string();
                    let args = func["arguments"].clone();
                    let call_id = format!("call_{}", &uuid::Uuid::new_v4().to_string().replace('-', "")[..8]);
                    tool_calls.push(ToolCallItem { id: call_id, name, arguments: args });
                }
            }
            if data["done"].as_bool().unwrap_or(false) {
                prompt_tokens = data["prompt_eval_count"].as_u64().unwrap_or(0) as usize;
                completion_tokens = data["eval_count"].as_u64().unwrap_or(0) as usize;
                done_reason = data["done_reason"].as_str().unwrap_or("").to_string();
            }
        }

        if tool_calls.is_empty() && !done_reason.is_empty() && done_reason != "stop" {
            println!("  ⚠️ [OLLAMA] Réponse sans tool_call (done_reason={})", done_reason);
        }

        Ok(ChatResponse {
            content,
            tool_calls,
            prompt_tokens,
            completion_tokens,
            total_tokens: prompt_tokens + completion_tokens,
            reasoning_content: None,
        })
    }

    fn chat_openai(
        &self,
        messages: &[ChatMessage],
        tools: Option<&[Value]>,
        think_effort: &str,
        on_token: &mut dyn FnMut(&str, &str),
    ) -> Result<ChatResponse, String> {
        let url = format!("{}/v1/chat/completions", self.base_url);
        let mut body = serde_json::json!({
            "model": self.model,
            "messages": messages,
            "temperature": 0.2,
            "stream": true,
            "stream_options": { "include_usage": true }
        });

        let url_low = self.base_url.to_lowercase();
        let model_low = self.model.to_lowercase();
        if url_low.contains("deepseek") || model_low.contains("deepseek") {
            if think_effort == "off" || think_effort == "none" || think_effort == "false" {
                body["thinking"] = serde_json::json!({ "type": "disabled" });
            } else {
                body["thinking"] = serde_json::json!({ "type": "enabled" });
                body.as_object_mut().map(|o| o.remove("temperature"));
            }
        } else if think_effort != "off" && (model_low.starts_with("o1") || model_low.starts_with("o3")) {
            body["reasoning_effort"] = serde_json::json!(think_effort);
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

        let reader = BufReader::new(resp);
        let mut content = String::new();
        let mut reasoning = String::new();
        let mut finish_reason = String::new();
        let mut prompt_tokens = 0usize;
        let mut completion_tokens = 0usize;
        // index -> (id, name, arguments_buffer)
        let mut tc_acc: std::collections::BTreeMap<usize, (String, String, String)> = std::collections::BTreeMap::new();

        for line in reader.lines() {
            if CANCEL_LLM.load(Ordering::SeqCst) {
                return Err("⏹️ Stream annulé par l'utilisateur.".to_string());
            }
            let line = line.map_err(|e| format!("Erreur lecture stream OpenAI: {}", e))?;
            let trimmed = line.trim();
            if trimmed.is_empty() || !trimmed.starts_with("data:") {
                continue;
            }
            let payload_str = trimmed.trim_start_matches("data:").trim();
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
            let choice0 = &data["choices"][0];
            if let Some(fr) = choice0["finish_reason"].as_str() {
                if !fr.is_empty() {
                    finish_reason = fr.to_string();
                }
            }
            let delta = &choice0["delta"];
            if let Some(rc) = delta["reasoning_content"].as_str() {
                if !rc.is_empty() {
                    on_token("thinking", rc);
                    reasoning.push_str(rc);
                }
            }
            if let Some(c) = delta["content"].as_str() {
                if !c.is_empty() {
                    on_token("content", c);
                    content.push_str(c);
                }
            }
            if let Some(arr) = delta["tool_calls"].as_array() {
                for tc in arr {
                    let idx = tc["index"].as_u64().unwrap_or(0) as usize;
                    let entry = tc_acc.entry(idx).or_insert_with(|| (String::new(), String::new(), String::new()));
                    if let Some(id) = tc["id"].as_str() {
                        if !id.is_empty() { entry.0 = id.to_string(); }
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

        if finish_reason == "length" {
            println!("  ⚠️ [OPENAI] Réponse tronquée (finish_reason=length)");
        }

        let mut tool_calls = Vec::new();
        for (_idx, (call_id, name, raw_args)) in tc_acc {
            let args_to_parse = if raw_args.trim().is_empty() { "{}" } else { raw_args.as_str() };
            let parsed_args: Value = match serde_json::from_str(args_to_parse) {
                Ok(v) => v,
                Err(_) => {
                    println!("  ⚠️ [OPENAI] Tool '{}' arguments tronqués (finish_reason={}, {} chars)", name, finish_reason, raw_args.len());
                    serde_json::json!({
                        "__truncated_error": format!("Arguments JSON tronqués ({} chars, finish_reason={}). Utilise stage_only: true et écris UN seul fichier à la fois.", raw_args.len(), finish_reason)
                    })
                }
            };
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

