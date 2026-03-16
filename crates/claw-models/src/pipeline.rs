use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A pipeline template: a reusable sequence of steps.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pipeline {
    pub id: Uuid,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub workspace_id: Option<Uuid>,
    pub steps: Vec<PipelineStep>,
    pub created_at: DateTime<Utc>,
}

/// A single step in a pipeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineStep {
    pub name: String,
    pub template_id: Option<Uuid>,
    /// Inline prompt (used if no template_id, or as override with {{previous_result}}).
    #[serde(default)]
    pub prompt: String,
    #[serde(default)]
    pub skill_ids: Vec<String>,
    pub model: Option<String>,
    pub timeout_secs: Option<u64>,
}

/// A running or completed pipeline execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineRun {
    pub id: Uuid,
    pub pipeline_id: Uuid,
    pub pipeline_name: String,
    pub workspace_id: Option<Uuid>,
    pub status: PipelineStatus,
    /// Job IDs for each step (filled as steps execute).
    #[serde(default)]
    pub step_jobs: Vec<Option<Uuid>>,
    pub current_step: usize,
    pub created_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PipelineStatus {
    Pending,
    Running,
    Completed,
    Failed,
}

impl Default for PipelineStatus {
    fn default() -> Self {
        Self::Pending
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreatePipelineRequest {
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub workspace_id: Option<Uuid>,
    pub steps: Vec<PipelineStep>,
}
