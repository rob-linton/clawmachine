use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum WorkspacePersistence {
    Ephemeral,
    Persistent,
    Snapshot,
}

impl Default for WorkspacePersistence {
    fn default() -> Self {
        Self::Persistent
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workspace {
    pub id: Uuid,
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// Legacy field — only set for old-style workspaces with direct disk paths.
    /// New workspaces use bare repos at ~/.claw/repos/{id}.git (path derived, not stored).
    #[serde(default)]
    pub path: Option<PathBuf>,
    #[serde(default)]
    pub skill_ids: Vec<String>,
    #[serde(default)]
    pub tool_ids: Vec<String>,
    /// Maps tool_id → credential_id for injecting credentials at job time.
    #[serde(default)]
    pub credential_bindings: HashMap<String, String>,
    pub claude_md: Option<String>,
    #[serde(default)]
    pub persistence: WorkspacePersistence,
    #[serde(default)]
    pub remote_url: Option<String>,
    #[serde(default)]
    pub base_image: Option<String>,
    /// Per-workspace resource limit overrides (Docker mode only).
    #[serde(default)]
    pub memory_limit: Option<String>,
    #[serde(default)]
    pub cpu_limit: Option<f64>,
    #[serde(default)]
    pub network_mode: Option<String>,
    /// Lineage: which workspace this was forked from.
    #[serde(default)]
    pub parent_workspace_id: Option<Uuid>,
    /// Git ref (commit hash, tag, or branch) in the parent at fork time.
    #[serde(default)]
    pub parent_ref: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Workspace {
    /// Returns true if this is a legacy workspace with a direct disk path.
    pub fn is_legacy(&self) -> bool {
        self.path.is_some()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateWorkspaceRequest {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    /// If set, creates a legacy workspace at this path. Otherwise uses bare repo.
    pub path: Option<PathBuf>,
    #[serde(default)]
    pub skill_ids: Vec<String>,
    #[serde(default)]
    pub tool_ids: Vec<String>,
    #[serde(default)]
    pub credential_bindings: HashMap<String, String>,
    pub claude_md: Option<String>,
    #[serde(default)]
    pub persistence: Option<WorkspacePersistence>,
    #[serde(default)]
    pub remote_url: Option<String>,
    #[serde(default)]
    pub base_image: Option<String>,
    #[serde(default)]
    pub memory_limit: Option<String>,
    #[serde(default)]
    pub cpu_limit: Option<f64>,
    #[serde(default)]
    pub network_mode: Option<String>,
    /// Fork from an existing workspace.
    #[serde(default)]
    pub parent_workspace_id: Option<Uuid>,
    /// Git ref in the parent to fork from (defaults to HEAD).
    #[serde(default)]
    pub parent_ref: Option<String>,
}

/// Application-level event for workspace history timeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceEvent {
    pub timestamp: DateTime<Utc>,
    pub event_type: WorkspaceEventType,
    /// Related entity ID (job_id, workspace_id, commit hash).
    pub related_id: Option<String>,
    /// Human-readable description.
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceEventType {
    Initialized,
    Forked,
    JobStarted,
    JobCompleted,
    JobFailed,
    SnapshotPromoted,
    FileModified,
    Synced,
    Reverted,
    ChildForked,
}
