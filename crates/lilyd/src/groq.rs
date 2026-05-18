use anyhow::{Context, Result};
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::Value;

const ENDPOINT: &str = "https://api.groq.com/openai/v1/chat/completions";

#[derive(Debug, Clone, Serialize)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<Value>,
    pub tools: Value,
    pub tool_choice: String,
    pub temperature: f32,
    pub max_tokens: u32,
    pub service_tier: String,
    pub stream: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ChatResponse {
    pub choices: Vec<Choice>,
    #[serde(default)]
    pub usage: Option<Usage>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Choice {
    pub message: AssistantMessage,
    #[serde(default)]
    #[allow(dead_code)]
    pub finish_reason: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AssistantMessage {
    pub role: String,
    #[serde(default)]
    pub content: Option<Value>,
    #[serde(default)]
    pub tool_calls: Option<Vec<ToolCall>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type", default = "default_tool_type")]
    pub kind: String,
    pub function: FunctionCall,
}

fn default_tool_type() -> String { "function".to_string() }

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FunctionCall {
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Usage {
    #[serde(default)]
    pub prompt_tokens: u32,
    #[serde(default)]
    pub completion_tokens: u32,
    #[serde(default)]
    #[allow(dead_code)]
    pub total_tokens: u32,
}

pub async fn chat(
    http: &reqwest::Client,
    api_key: &str,
    req: &ChatRequest,
) -> Result<ChatResponse> {
    let resp = http
        .post(ENDPOINT)
        .bearer_auth(api_key)
        .json(req)
        .send()
        .await
        .context("POST groq chat/completions")?;
    let status = resp.status();
    let body = resp.text().await.context("read groq response")?;
    if !status.is_success() {
        anyhow::bail!("groq {status}: {body}");
    }
    let parsed: ChatResponse = serde_json::from_str(&body)
        .with_context(|| format!("decode groq response: {body}"))?;
    Ok(parsed)
}

/// Build a user message containing a screenshot for the model to inspect.
pub fn image_user_message(prefix: &str, png: &[u8]) -> Value {
    let b64 = base64::engine::general_purpose::STANDARD.encode(png);
    serde_json::json!({
        "role": "user",
        "content": [
            { "type": "text", "text": prefix },
            { "type": "image_url", "image_url": {
                "url": format!("data:image/png;base64,{b64}")
            }}
        ]
    })
}

pub fn text_message(role: &str, text: impl Into<String>) -> Value {
    serde_json::json!({ "role": role, "content": text.into() })
}

pub fn tool_result_message(tool_call_id: &str, content: &str) -> Value {
    serde_json::json!({
        "role": "tool",
        "tool_call_id": tool_call_id,
        "content": content,
    })
}

pub fn assistant_message_to_json(m: &AssistantMessage) -> Value {
    let mut obj = serde_json::Map::new();
    obj.insert("role".into(), Value::String("assistant".into()));
    obj.insert(
        "content".into(),
        m.content.clone().unwrap_or(Value::String("".into())),
    );
    if let Some(tcs) = &m.tool_calls {
        obj.insert("tool_calls".into(), serde_json::to_value(tcs).unwrap_or(Value::Null));
    }
    Value::Object(obj)
}
