use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A credential set for tool authentication.
/// Values are stored encrypted in Redis — this struct only holds metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Credential {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// Key names only (e.g., ["AWS_ACCESS_KEY_ID", "AWS_SECRET_ACCESS_KEY"]).
    /// Actual values are stored separately and encrypted.
    #[serde(default)]
    pub keys: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Request to create or update a credential (includes plaintext values for storage).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateCredentialRequest {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// Key-value pairs (e.g., {"AWS_ACCESS_KEY_ID": "AKIA...", "AWS_SECRET_ACCESS_KEY": "..."}).
    /// Values are encrypted before storage; never returned in GET responses.
    pub values: HashMap<String, String>,
}

/// Response for credential listing — values are masked.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CredentialResponse {
    pub id: String,
    pub name: String,
    pub description: String,
    pub keys: Vec<String>,
    /// Masked values: {"AWS_ACCESS_KEY_ID": "***set***", ...}
    pub masked_values: HashMap<String, String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
