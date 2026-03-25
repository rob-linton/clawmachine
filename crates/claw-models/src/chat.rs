use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A chat session tied to a user and a persistent workspace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatSession {
    pub id: Uuid,
    pub user_id: String,
    pub workspace_id: Uuid,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default = "default_model")]
    pub model: String,
    #[serde(default = "default_context_window")]
    pub context_window_size: u32,
    #[serde(default)]
    pub total_messages: u32,
    #[serde(default)]
    pub total_cost_usd: f64,
    #[serde(default)]
    pub skill_ids: Vec<String>,
    #[serde(default)]
    pub tool_ids: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_activity: DateTime<Utc>,
}

fn default_model() -> String {
    "sonnet".to_string()
}

fn default_context_window() -> u32 {
    20
}

/// A single message in a chat session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub seq: u32,
    pub role: String,
    pub content: String,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub job_id: Option<Uuid>,
    #[serde(default)]
    pub cost_usd: Option<f64>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub token_estimate: u32,
    #[serde(default)]
    pub files_written: Vec<String>,
    pub timestamp: DateTime<Utc>,
}

/// Request to send a new chat message.
#[derive(Debug, Clone, Deserialize)]
pub struct SendMessageRequest {
    pub content: String,
    #[serde(default)]
    pub model: Option<String>,
}
