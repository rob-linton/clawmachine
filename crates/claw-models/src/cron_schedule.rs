use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

use crate::OutputDest;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronSchedule {
    pub id: Uuid,
    pub name: String,
    pub schedule: String,
    pub enabled: bool,
    pub prompt: String,
    #[serde(default)]
    pub skill_ids: Vec<String>,
    #[serde(default = "default_cron_working_dir")]
    pub working_dir: PathBuf,
    pub model: Option<String>,
    pub max_budget_usd: Option<f64>,
    #[serde(default)]
    pub output_dest: OutputDest,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default = "default_cron_priority")]
    pub priority: u8,
    pub workspace_id: Option<Uuid>,
    pub template_id: Option<Uuid>,
    pub last_run: Option<DateTime<Utc>>,
    pub last_job_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
}

fn default_cron_working_dir() -> PathBuf {
    PathBuf::from(".")
}

fn default_cron_priority() -> u8 {
    5
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateCronRequest {
    pub name: String,
    pub schedule: String,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    pub prompt: String,
    #[serde(default)]
    pub skill_ids: Vec<String>,
    pub working_dir: Option<PathBuf>,
    pub model: Option<String>,
    pub max_budget_usd: Option<f64>,
    #[serde(default)]
    pub output_dest: OutputDest,
    #[serde(default)]
    pub tags: Vec<String>,
    pub priority: Option<u8>,
    pub workspace_id: Option<Uuid>,
    pub template_id: Option<Uuid>,
}

fn default_enabled() -> bool {
    true
}
