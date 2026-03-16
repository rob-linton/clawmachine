use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workspace {
    pub id: Uuid,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub path: PathBuf,
    #[serde(default)]
    pub skill_ids: Vec<String>,
    pub claude_md: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateWorkspaceRequest {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub path: Option<PathBuf>,
    #[serde(default)]
    pub skill_ids: Vec<String>,
    pub claude_md: Option<String>,
}
