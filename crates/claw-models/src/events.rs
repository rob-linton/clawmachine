use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::JobStatus;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WsServerMessage {
    JobUpdate {
        job_id: Uuid,
        status: JobStatus,
        worker_id: Option<String>,
        timestamp: DateTime<Utc>,
    },
    JobLog {
        job_id: Uuid,
        line: String,
        timestamp: DateTime<Utc>,
    },
    Stats {
        pending: u64,
        running: u64,
        completed_today: u64,
        failed_today: u64,
        total_cost_today: f64,
    },
    Error {
        message: String,
    },
}
