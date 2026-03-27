//! Per-user persistent notebook (memory) stored in Redis.
//! Survives chat deletion, container restarts, and server reboots.

use chrono::{DateTime, Utc};
use deadpool_redis::Pool;
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};

use crate::RedisError;

/// A single entry in the user's notebook.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotebookEntry {
    pub content: String,
    pub summary: String,
    pub created: DateTime<Utc>,
    pub updated: DateTime<Utc>,
    pub access_count: u32,
    pub last_accessed: DateTime<Utc>,
}

/// Metadata about a user's notebook (mood history, anticipation, consolidation state).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NotebookMeta {
    pub total_entries: u32,
    pub last_consolidation: Option<DateTime<Utc>>,
    #[serde(default)]
    pub mood_history: Vec<MoodEntry>,
    #[serde(default)]
    pub anticipation: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MoodEntry {
    pub mood: String,
    pub timestamp: DateTime<Utc>,
}

fn notebook_set_key(username: &str) -> String {
    format!("claw:user:{}:notebook", username)
}

fn notebook_entry_key(username: &str, path: &str) -> String {
    format!("claw:user:{}:notebook:{}", username, path)
}

fn notebook_meta_key(username: &str) -> String {
    format!("claw:user:{}:notebook_meta", username)
}

/// Validate a notebook path to prevent path traversal.
/// Rejects paths containing `..`, absolute paths, and paths with null bytes.
pub fn validate_notebook_path(path: &str) -> Result<(), String> {
    if path.is_empty() {
        return Err("Empty path".to_string());
    }
    if path.contains("..") {
        return Err(format!("Path traversal rejected: {}", path));
    }
    if path.starts_with('/') || path.starts_with('\\') {
        return Err(format!("Absolute path rejected: {}", path));
    }
    if path.contains('\0') {
        return Err("Null byte in path".to_string());
    }
    // Only allow alphanumeric, hyphens, underscores, dots, and forward slashes
    if !path.chars().all(|c| c.is_alphanumeric() || "-_./".contains(c)) {
        return Err(format!("Invalid characters in path: {}", path));
    }
    Ok(())
}

/// List all notebook file paths for a user.
pub async fn list_notebook_files(pool: &Pool, username: &str) -> Result<Vec<String>, RedisError> {
    let mut conn = pool.get().await?;
    let files: Vec<String> = conn.smembers(&notebook_set_key(username)).await?;
    Ok(files)
}

/// Get a single notebook entry.
pub async fn get_notebook_entry(pool: &Pool, username: &str, path: &str) -> Result<Option<NotebookEntry>, RedisError> {
    validate_notebook_path(path).map_err(|e| RedisError::Other(e))?;
    let mut conn = pool.get().await?;
    let raw: Option<String> = conn.get(&notebook_entry_key(username, path)).await?;
    match raw {
        Some(json) => Ok(serde_json::from_str(&json)?),
        None => Ok(None),
    }
}

/// Create or update a notebook entry.
pub async fn upsert_notebook_entry(pool: &Pool, username: &str, path: &str, entry: &NotebookEntry) -> Result<(), RedisError> {
    validate_notebook_path(path).map_err(|e| RedisError::Other(e))?;
    let mut conn = pool.get().await?;
    let json = serde_json::to_string(entry)?;
    let _: () = conn.set(&notebook_entry_key(username, path), &json).await?;
    let _: () = conn.sadd(&notebook_set_key(username), path).await?;
    Ok(())
}

/// Delete a notebook entry.
pub async fn delete_notebook_entry(pool: &Pool, username: &str, path: &str) -> Result<(), RedisError> {
    validate_notebook_path(path).map_err(|e| RedisError::Other(e))?;
    let mut conn = pool.get().await?;
    let _: () = conn.del(&notebook_entry_key(username, path)).await?;
    let _: () = conn.srem(&notebook_set_key(username), path).await?;
    Ok(())
}

/// Get notebook metadata.
pub async fn get_notebook_meta(pool: &Pool, username: &str) -> Result<Option<NotebookMeta>, RedisError> {
    let mut conn = pool.get().await?;
    let raw: Option<String> = conn.get(&notebook_meta_key(username)).await?;
    match raw {
        Some(json) => Ok(serde_json::from_str(&json)?),
        None => Ok(None),
    }
}

/// Set notebook metadata.
pub async fn set_notebook_meta(pool: &Pool, username: &str, meta: &NotebookMeta) -> Result<(), RedisError> {
    let mut conn = pool.get().await?;
    let json = serde_json::to_string(meta)?;
    let _: () = conn.set(&notebook_meta_key(username), &json).await?;
    Ok(())
}

/// Bump access_count and last_accessed on a notebook entry.
pub async fn touch_notebook_entry(pool: &Pool, username: &str, path: &str) -> Result<(), RedisError> {
    if let Some(mut entry) = get_notebook_entry(pool, username, path).await? {
        entry.access_count += 1;
        entry.last_accessed = Utc::now();
        upsert_notebook_entry(pool, username, path, &entry).await?;
    }
    Ok(())
}

/// Append a mood entry to the notebook meta (keeps last 50).
pub async fn append_mood(pool: &Pool, username: &str, mood: &str) -> Result<(), RedisError> {
    let mut meta = get_notebook_meta(pool, username).await?.unwrap_or_default();
    meta.mood_history.push(MoodEntry {
        mood: mood.to_string(),
        timestamp: Utc::now(),
    });
    // Keep last 50 entries
    if meta.mood_history.len() > 50 {
        meta.mood_history = meta.mood_history.split_off(meta.mood_history.len() - 50);
    }
    set_notebook_meta(pool, username, &meta).await
}

/// Update the anticipation note in notebook meta.
pub async fn update_anticipation(pool: &Pool, username: &str, anticipation: &str) -> Result<(), RedisError> {
    let mut meta = get_notebook_meta(pool, username).await?.unwrap_or_default();
    meta.anticipation = Some(anticipation.to_string());
    set_notebook_meta(pool, username, &meta).await
}

/// Get the most recent N mood strings.
pub async fn get_recent_moods(pool: &Pool, username: &str, n: usize) -> Result<Vec<String>, RedisError> {
    let meta = get_notebook_meta(pool, username).await?.unwrap_or_default();
    let moods: Vec<String> = meta.mood_history.iter()
        .rev()
        .take(n)
        .map(|m| m.mood.clone())
        .collect();
    Ok(moods)
}

/// Build a notebook index string (path — summary, one per line) for prompt injection.
pub async fn build_notebook_index(pool: &Pool, username: &str) -> Result<String, RedisError> {
    let files = list_notebook_files(pool, username).await?;
    let mut lines = Vec::new();
    for path in &files {
        if let Some(entry) = get_notebook_entry(pool, username, path).await? {
            lines.push(format!("- {} — {}", path, entry.summary));
        }
    }
    Ok(lines.join("\n"))
}

/// Score a notebook entry for importance ranking.
/// Higher = more important = should go in CLAUDE.md hot tier.
pub fn score_entry(entry: &NotebookEntry, path: &str) -> f64 {
    let now = Utc::now();
    let days_since_access = (now - entry.last_accessed).num_hours().max(1) as f64 / 24.0;
    let recency = 1.0 / (1.0 + days_since_access);
    let frequency = (1.0 + entry.access_count as f64).ln();
    let type_weight = if path.starts_with("about-user") {
        3.0
    } else if path.starts_with("active-project") || path.starts_with("decisions") {
        2.5
    } else if path.starts_with("preferences") || path.starts_with("people") {
        2.0
    } else if path.starts_with("topics/") {
        1.5
    } else {
        1.0
    };
    recency * frequency * type_weight
}
