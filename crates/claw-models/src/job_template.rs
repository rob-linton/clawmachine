use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::OutputDest;

/// A reusable job definition that can be referenced by jobs, crons, and pipelines.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobTemplate {
    pub id: Uuid,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub prompt: String,
    #[serde(default)]
    pub skill_ids: Vec<String>,
    #[serde(default)]
    pub tool_ids: Vec<String>,
    pub workspace_id: Option<Uuid>,
    pub model: Option<String>,
    pub timeout_secs: Option<u64>,
    pub allowed_tools: Option<Vec<String>>,
    #[serde(default)]
    pub output_dest: OutputDest,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default = "default_template_priority")]
    pub priority: u8,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

fn default_template_priority() -> u8 {
    5
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateJobTemplateRequest {
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub prompt: String,
    #[serde(default)]
    pub skill_ids: Vec<String>,
    #[serde(default)]
    pub tool_ids: Vec<String>,
    pub workspace_id: Option<Uuid>,
    pub model: Option<String>,
    pub timeout_secs: Option<u64>,
    pub allowed_tools: Option<Vec<String>>,
    #[serde(default)]
    pub output_dest: OutputDest,
    #[serde(default)]
    pub tags: Vec<String>,
    pub priority: Option<u8>,
}
