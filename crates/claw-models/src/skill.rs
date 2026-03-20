use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A skill is a directory of files deployed to .claude/skills/{id}/.
/// The `content` field holds the SKILL.md content.
/// The `files` field holds all other bundled files (scripts, references, assets).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Skill {
    pub id: String,
    pub name: String,
    pub content: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub tags: Vec<String>,
    /// Bundled files: keys are relative paths (e.g. "scripts/run.sh"), values are text content.
    #[serde(default)]
    pub files: HashMap<String, String>,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub license: Option<String>,
    #[serde(default)]
    pub source_url: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
