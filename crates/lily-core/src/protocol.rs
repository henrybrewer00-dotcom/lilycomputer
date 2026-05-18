use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunRequest {
    pub prompt: String,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub reset: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunResponse {
    pub session_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CancelRequest {
    pub session_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthResponse {
    pub ok: bool,
    pub version: String,
    pub uptime_s: u64,
    pub model: String,
    #[serde(default)]
    pub groq_configured: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetKeyRequest {
    pub groq_api_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    Status { state: String },
    UserPrompt { text: String },
    ToolCall { id: String, name: String, args: serde_json::Value },
    ToolResult { id: String, ok: bool, summary: String, elapsed_ms: u64 },
    Screenshot { path: String, index: u32 },
    Assistant { text: String },
    Tokens { prompt: u32, completion: u32, total: u32 },
    Done { summary: String },
    Error { message: String },
}
