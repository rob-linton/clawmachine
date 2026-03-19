use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Job {
    pub id: Uuid,
    pub status: JobStatus,
    pub prompt: String,
    #[serde(default)]
    pub skill_ids: Vec<String>,
    #[serde(default)]
    pub skill_tags: Vec<String>,
    #[serde(default)]
    pub tool_ids: Vec<String>,
    #[serde(default = "default_working_dir")]
    pub working_dir: PathBuf,
    pub model: Option<String>,
    pub max_budget_usd: Option<f64>,
    pub allowed_tools: Option<Vec<String>>,
    #[serde(default)]
    pub output_dest: OutputDest,
    #[serde(default)]
    pub source: JobSource,
    #[serde(default = "default_priority")]
    pub priority: u8,
    #[serde(default)]
    pub tags: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub worker_id: Option<String>,
    pub error: Option<String>,
    pub cost_usd: Option<f64>,
    pub duration_ms: Option<u64>,
    #[serde(default)]
    pub retry_count: u32,
    pub timeout_secs: Option<u64>,
    pub workspace_id: Option<Uuid>,
    pub cron_id: Option<Uuid>,
    pub template_id: Option<Uuid>,
    pub pipeline_run_id: Option<Uuid>,
    pub pipeline_step: Option<usize>,
    pub skill_snapshot: Option<serde_json::Value>,
    pub assembled_prompt: Option<String>,
}

fn default_priority() -> u8 {
    5
}

fn default_working_dir() -> PathBuf {
    PathBuf::from(".")
}

/// Request body for creating a new job via the API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateJobRequest {
    pub prompt: String,
    #[serde(default)]
    pub skill_ids: Vec<String>,
    #[serde(default)]
    pub skill_tags: Vec<String>,
    #[serde(default)]
    pub tool_ids: Vec<String>,
    pub working_dir: Option<PathBuf>,
    pub model: Option<String>,
    pub max_budget_usd: Option<f64>,
    pub allowed_tools: Option<Vec<String>>,
    #[serde(default)]
    pub output_dest: OutputDest,
    #[serde(default)]
    pub tags: Vec<String>,
    pub priority: Option<u8>,
    pub timeout_secs: Option<u64>,
    pub workspace_id: Option<Uuid>,
    pub template_id: Option<Uuid>,
}

/// Summary returned after submitting a job.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateJobResponse {
    pub id: Uuid,
    pub status: JobStatus,
    pub created_at: DateTime<Utc>,
}

/// Response for job result queries.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobResultResponse {
    pub job_id: Uuid,
    pub result: String,
    pub cost_usd: f64,
    pub duration_ms: u64,
}

/// Queue status overview.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueStatus {
    pub pending: u64,
    pub running: u64,
    pub completed: u64,
    pub failed: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, strum::Display, strum::EnumString)]
#[strum(serialize_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum JobStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
}

impl Default for JobStatus {
    fn default() -> Self {
        Self::Pending
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum OutputDest {
    Redis,
    File { path: PathBuf },
    Webhook { url: String },
}

impl Default for OutputDest {
    fn default() -> Self {
        Self::Redis
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, strum::Display, strum::EnumString)]
#[strum(serialize_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum JobSource {
    Cli,
    Api,
    Cron,
    #[strum(serialize = "filewatcher")]
    FileWatcher,
}

impl Default for JobSource {
    fn default() -> Self {
        Self::Api
    }
}
