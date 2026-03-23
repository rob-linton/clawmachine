use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// A provisioned CLI tool (e.g., az-cli, aws-cli) that can be installed into
/// Docker sandbox images and made available during job execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tool {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub tags: Vec<String>,
    /// Shell commands to install the tool (targets Docker/Debian).
    /// Multiline: each line is joined with ` && ` for Dockerfile RUN directives.
    pub install_commands: String,
    /// Command to check if tool is already installed (exit 0 = present).
    /// Example: "az --version" or "aws --version"
    pub check_command: String,
    /// Environment variables this tool needs at runtime (documentation + credential binding).
    #[serde(default)]
    pub env_vars: Vec<ToolEnvVar>,
    /// Optional login/auth commands run before claude -p in the container.
    /// These can reference credential env vars injected at runtime.
    #[serde(default)]
    pub auth_script: Option<String>,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub license: Option<String>,
    #[serde(default)]
    pub source_url: Option<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Declares an environment variable a tool needs at runtime.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolEnvVar {
    /// Env var name (e.g., "AZURE_CLIENT_ID").
    pub key: String,
    /// Human-readable description.
    #[serde(default)]
    pub description: String,
    /// Whether this variable is required for the tool to function.
    #[serde(default = "default_true")]
    pub required: bool,
}

fn default_true() -> bool {
    true
}

/// Request body for creating a new tool via the API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateToolRequest {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub tags: Vec<String>,
    pub install_commands: String,
    pub check_command: String,
    #[serde(default)]
    pub env_vars: Vec<ToolEnvVar>,
    #[serde(default)]
    pub auth_script: Option<String>,
}
