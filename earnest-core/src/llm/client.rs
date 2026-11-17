use std::time::Duration;
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;

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
    ) -> Result<ChatResponse, String> {
        if self.provider_type == "ollama" {
            self.chat_ollama(messages, tools)
        } else {
            self.chat_openai(messages, tools)
        }
    }

    fn chat_ollama(
        &self,
        messages: &[ChatMessage],
        tools: Option<&[Value]>,
    ) -> Result<ChatResponse, String> {
        let url = format!("{}/api/chat", self.base_url);
        let mut body = serde_json::json!({
            "model": self.model,
            "messages": messages,
            "stream": false,
            "think": false
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

        let data: Value = resp.json().map_err(|e| format!("Erreur parsing JSON Ollama: {}", e))?;
        let msg = &data["message"];
        let content = msg["content"].as_str().unwrap_or("").to_string();

        let mut tool_calls = Vec::new();
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

        let prompt_tokens = data["prompt_eval_count"].as_u64().unwrap_or(0) as usize;
        let completion_tokens = data["eval_count"].as_u64().unwrap_or(0) as usize;

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
    ) -> Result<ChatResponse, String> {
        let url = format!("{}/v1/chat/completions", self.base_url);
        let mut body = serde_json::json!({
            "model": self.model,
            "messages": messages,
            "temperature": 0.2
        });

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

        let data: Value = resp.json().map_err(|e| format!("Erreur parsing JSON OpenAI: {}", e))?;
        let choice = &data["choices"][0]["message"];
        let content = choice["content"].as_str().unwrap_or("").to_string();
        let reasoning = choice["reasoning_content"].as_str().map(|s| s.to_string());

        let mut tool_calls = Vec::new();
        if let Some(arr) = choice["tool_calls"].as_array() {
            for call in arr {
                let func = &call["function"];
                let name = func["name"].as_str().unwrap_or("").to_string();
                let raw_args = func["arguments"].as_str().unwrap_or("{}");
                let parsed_args = parse_tool_args_with_repair(&name, raw_args);
                let call_id = call["id"].as_str().unwrap_or("call_gen").to_string();
                tool_calls.push(ToolCallItem { id: call_id, name, arguments: parsed_args });
            }
        }

        let usage = &data["usage"];
        let prompt_tokens = usage["prompt_tokens"].as_u64().unwrap_or(0) as usize;
        let completion_tokens = usage["completion_tokens"].as_u64().unwrap_or(0) as usize;

        Ok(ChatResponse {
            content,
            tool_calls,
            prompt_tokens,
            completion_tokens,
            total_tokens: prompt_tokens + completion_tokens,
            reasoning_content: reasoning,
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

