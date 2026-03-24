use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::post,
    Json, Router,
};
use deadpool_redis::Pool;
use serde::Serialize;
use std::collections::HashMap;

use crate::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/catalog/sync", post(sync_catalog))
}

#[derive(Debug, Clone, Serialize)]
pub struct SyncSummary {
    pub skills_installed: u32,
    pub skills_updated: u32,
    pub skills_skipped: u32,
    pub tools_installed: u32,
    pub tools_updated: u32,
    pub tools_skipped: u32,
}

async fn sync_catalog(State(state): State<AppState>) -> impl IntoResponse {
    match do_sync(&state.pool).await {
        Ok(summary) => (StatusCode::OK, Json(serde_json::json!(summary))).into_response(),
        Err(e) => {
            tracing::warn!(error = %e, "Catalog sync failed");
            (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": e})),
            )
                .into_response()
        }
    }
}

/// Core sync logic, callable from both the HTTP handler and the startup background task.
pub async fn do_sync(pool: &Pool) -> Result<SyncSummary, String> {
    // 1. Read catalog_url from config
    let catalog_url = claw_redis::get_config(pool, "catalog_url")
        .await
        .map_err(|e| format!("Failed to read catalog_url config: {e}"))?;

    if catalog_url.is_empty() {
        return Err("No catalog_url configured".into());
    }

    // 2. Determine git repo URL from catalog_url
    let repo_url = derive_repo_url(&catalog_url)?;

    // 3. Clone the repo to a temp directory
    let temp_dir =
        tempfile::tempdir().map_err(|e| format!("Failed to create temp dir: {e}"))?;
    let temp_path = temp_dir.path();

    let clone_result = tokio::time::timeout(
        std::time::Duration::from_secs(30),
        tokio::process::Command::new("git")
            .args([
                "clone",
                "--depth",
                "1",
                "--single-branch",
                &repo_url,
                &temp_path.to_string_lossy(),
            ])
            .output(),
    )
    .await
    .map_err(|_| "git clone timed out (30s)".to_string())?
    .map_err(|e| format!("git clone failed: {e}"))?;

    if !clone_result.status.success() {
        let stderr = String::from_utf8_lossy(&clone_result.stderr);
        return Err(format!("git clone failed: {stderr}"));
    }

    // 4. Read and parse catalog.json
    let catalog_path = temp_path.join("catalog.json");
    let catalog_text = tokio::fs::read_to_string(&catalog_path)
        .await
        .map_err(|e| format!("Failed to read catalog.json: {e}"))?;

    let catalog: serde_json::Value = serde_json::from_str(&catalog_text)
        .map_err(|e| format!("Failed to parse catalog.json: {e}"))?;

    let mut summary = SyncSummary {
        skills_installed: 0,
        skills_updated: 0,
        skills_skipped: 0,
        tools_installed: 0,
        tools_updated: 0,
        tools_skipped: 0,
    };

    // 5. Sync skills
    if let Some(skills) = catalog.get("skills").and_then(|s| s.as_array()) {
        for entry in skills {
            let id = match entry.get("id").and_then(|v| v.as_str()) {
                Some(id) => id.to_string(),
                None => continue,
            };

            match sync_skill(pool, &id, entry, temp_path, &repo_url).await {
                Ok(SyncAction::Installed) => summary.skills_installed += 1,
                Ok(SyncAction::Updated) => summary.skills_updated += 1,
                Ok(SyncAction::Skipped) => summary.skills_skipped += 1,
                Err(e) => {
                    tracing::warn!(skill_id = %id, error = %e, "Failed to sync skill");
                    summary.skills_skipped += 1;
                }
            }
        }
    }

    // 6. Sync tools
    if let Some(tools) = catalog.get("tools").and_then(|s| s.as_array()) {
        for entry in tools {
            let id = match entry.get("id").and_then(|v| v.as_str()) {
                Some(id) => id.to_string(),
                None => continue,
            };

            match sync_tool(pool, &id, entry, temp_path, &repo_url).await {
                Ok(SyncAction::Installed) => summary.tools_installed += 1,
                Ok(SyncAction::Updated) => summary.tools_updated += 1,
                Ok(SyncAction::Skipped) => summary.tools_skipped += 1,
                Err(e) => {
                    tracing::warn!(tool_id = %id, error = %e, "Failed to sync tool");
                    summary.tools_skipped += 1;
                }
            }
        }
    }

    // 7. Cleanup happens automatically when temp_dir is dropped

    tracing::info!(
        skills_installed = summary.skills_installed,
        skills_updated = summary.skills_updated,
        skills_skipped = summary.skills_skipped,
        tools_installed = summary.tools_installed,
        tools_updated = summary.tools_updated,
        tools_skipped = summary.tools_skipped,
        "Catalog sync complete"
    );

    Ok(summary)
}

enum SyncAction {
    Installed,
    Updated,
    Skipped,
}

/// Derive the git clone URL from a catalog_url.
fn derive_repo_url(catalog_url: &str) -> Result<String, String> {
    if catalog_url.contains("raw.githubusercontent.com") {
        // e.g. https://raw.githubusercontent.com/rob-linton/claw-catalog/main/catalog.json
        // -> https://github.com/rob-linton/claw-catalog.git
        let parts: Vec<&str> = catalog_url
            .trim_start_matches("https://raw.githubusercontent.com/")
            .splitn(3, '/')
            .collect();
        if parts.len() < 2 {
            return Err(format!(
                "Cannot parse raw.githubusercontent.com URL: {catalog_url}"
            ));
        }
        Ok(format!(
            "https://github.com/{}/{}.git",
            parts[0], parts[1]
        ))
    } else if catalog_url.contains("github.com") {
        // Direct github.com URL — ensure .git suffix
        let base = catalog_url
            .trim_end_matches('/')
            .trim_end_matches(".git");
        // Strip any trailing path segments (e.g. /blob/main/catalog.json)
        let url = if let Some(idx) = base.find("/blob/") {
            &base[..idx]
        } else if let Some(idx) = base.find("/tree/") {
            &base[..idx]
        } else {
            base
        };
        Ok(format!("{url}.git"))
    } else {
        // Use the URL directly (assume it's a git-compatible URL)
        Ok(catalog_url.to_string())
    }
}

async fn sync_skill(
    pool: &Pool,
    id: &str,
    catalog_entry: &serde_json::Value,
    checkout_path: &std::path::Path,
    repo_url: &str,
) -> Result<SyncAction, String> {
    let catalog_version = catalog_entry
        .get("version")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    // Check if already installed
    let existing = claw_redis::get_skill(pool, id)
        .await
        .map_err(|e| format!("Redis error: {e}"))?;

    if let Some(ref skill) = existing {
        // If installed with a different source_url, skip (user-managed)
        let existing_source = skill.source_url.as_deref().unwrap_or("");
        if !existing_source.is_empty() && existing_source != repo_url {
            return Ok(SyncAction::Skipped);
        }
        // If same version, skip
        if skill.version == catalog_version && !catalog_version.is_empty() {
            return Ok(SyncAction::Skipped);
        }
        // If no source_url set (locally created), skip
        if existing_source.is_empty() {
            return Ok(SyncAction::Skipped);
        }
    }

    // Read skill files from checkout
    let skill_dir = checkout_path.join("skills").join(id);
    if !skill_dir.exists() {
        return Err(format!("Skill directory skills/{id} not found in checkout"));
    }

    let mut files = HashMap::new();
    read_dir_recursive(&skill_dir, &skill_dir, &mut files).await?;

    let manifest: Option<serde_json::Value> = files
        .get("manifest.json")
        .and_then(|s| serde_json::from_str(s).ok());

    let content = files.remove("SKILL.md").unwrap_or_default();
    files.remove("manifest.json");

    let name = catalog_entry
        .get("name")
        .and_then(|v| v.as_str())
        .or_else(|| manifest.as_ref().and_then(|m| m.get("name")).and_then(|v| v.as_str()))
        .unwrap_or(id)
        .to_string();

    let description = catalog_entry
        .get("description")
        .and_then(|v| v.as_str())
        .or_else(|| {
            manifest
                .as_ref()
                .and_then(|m| m.get("description"))
                .and_then(|v| v.as_str())
        })
        .unwrap_or("")
        .to_string();

    let tags: Vec<String> = catalog_entry
        .get("tags")
        .and_then(|t| serde_json::from_value::<Vec<String>>(t.clone()).ok())
        .or_else(|| {
            manifest
                .as_ref()
                .and_then(|m| m.get("tags"))
                .and_then(|t| serde_json::from_value::<Vec<String>>(t.clone()).ok())
        })
        .unwrap_or_default();

    let mut skill = claw_redis::new_skill(id, &name, &content, &description, tags, files);
    skill.source_url = Some(repo_url.to_string());

    if !catalog_version.is_empty() {
        skill.version = catalog_version.to_string();
    } else if let Some(ref m) = manifest {
        if let Some(v) = m.get("version").and_then(|v| v.as_str()) {
            skill.version = v.to_string();
        }
    }

    if let Some(ref m) = manifest {
        if let Some(v) = m.get("author").and_then(|v| v.as_str()) {
            skill.author = v.to_string();
        }
        if let Some(v) = m.get("license").and_then(|v| v.as_str()) {
            skill.license = Some(v.to_string());
        }
    }

    if let Some(ref existing_skill) = existing {
        // Update existing
        skill.created_at = existing_skill.created_at;
        claw_redis::update_skill(pool, &skill)
            .await
            .map_err(|e| format!("Failed to update skill: {e}"))?;
        Ok(SyncAction::Updated)
    } else {
        // Install new
        claw_redis::create_skill(pool, &skill)
            .await
            .map_err(|e| format!("Failed to create skill: {e}"))?;
        Ok(SyncAction::Installed)
    }
}

async fn sync_tool(
    pool: &Pool,
    id: &str,
    catalog_entry: &serde_json::Value,
    checkout_path: &std::path::Path,
    repo_url: &str,
) -> Result<SyncAction, String> {
    let catalog_version = catalog_entry
        .get("version")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    // Check if already installed
    let existing = claw_redis::get_tool(pool, id)
        .await
        .map_err(|e| format!("Redis error: {e}"))?;

    if let Some(ref tool) = existing {
        let existing_source = tool.source_url.as_deref().unwrap_or("");
        if !existing_source.is_empty() && existing_source != repo_url {
            return Ok(SyncAction::Skipped);
        }
        if tool.version == catalog_version && !catalog_version.is_empty() {
            return Ok(SyncAction::Skipped);
        }
        if existing_source.is_empty() {
            return Ok(SyncAction::Skipped);
        }
    }

    // Read tool files from checkout
    let tool_dir = checkout_path.join("tools").join(id);
    if !tool_dir.exists() {
        return Err(format!("Tool directory tools/{id} not found in checkout"));
    }

    let mut files = HashMap::new();
    read_dir_recursive(&tool_dir, &tool_dir, &mut files).await?;

    let manifest: Option<serde_json::Value> = files
        .get("manifest.json")
        .and_then(|s| serde_json::from_str(s).ok());

    let tool_json: Option<serde_json::Value> = files
        .remove("TOOL.json")
        .and_then(|s| serde_json::from_str(&s).ok());
    files.remove("manifest.json");

    let name = catalog_entry
        .get("name")
        .and_then(|v| v.as_str())
        .or_else(|| manifest.as_ref().and_then(|m| m.get("name")).and_then(|v| v.as_str()))
        .unwrap_or(id)
        .to_string();

    let description = catalog_entry
        .get("description")
        .and_then(|v| v.as_str())
        .or_else(|| {
            manifest
                .as_ref()
                .and_then(|m| m.get("description"))
                .and_then(|v| v.as_str())
        })
        .unwrap_or("")
        .to_string();

    let tags: Vec<String> = catalog_entry
        .get("tags")
        .and_then(|t| serde_json::from_value::<Vec<String>>(t.clone()).ok())
        .or_else(|| {
            manifest
                .as_ref()
                .and_then(|m| m.get("tags"))
                .and_then(|t| serde_json::from_value::<Vec<String>>(t.clone()).ok())
        })
        .unwrap_or_default();

    let install_commands = tool_json
        .as_ref()
        .and_then(|t| t.get("install_commands"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let check_command = tool_json
        .as_ref()
        .and_then(|t| t.get("check_command"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let mut tool = claw_redis::new_tool(id, &name, &install_commands, &check_command);
    tool.description = description;
    tool.tags = tags;
    tool.source_url = Some(repo_url.to_string());

    // Apply TOOL.json fields
    if let Some(ref tj) = tool_json {
        if let Some(env_vars) = tj.get("env_vars") {
            if let Ok(vars) =
                serde_json::from_value::<Vec<claw_models::ToolEnvVar>>(env_vars.clone())
            {
                tool.env_vars = vars;
            }
        }
        if let Some(auth) = tj.get("auth_script").and_then(|v| v.as_str()) {
            tool.auth_script = Some(auth.to_string());
        }
        if let Some(sc) = tj.get("skill_content").and_then(|v| v.as_str()) {
            tool.skill_content = Some(sc.to_string());
        }
    }

    if !catalog_version.is_empty() {
        tool.version = catalog_version.to_string();
    } else if let Some(ref m) = manifest {
        if let Some(v) = m.get("version").and_then(|v| v.as_str()) {
            tool.version = v.to_string();
        }
    }

    if let Some(ref m) = manifest {
        if let Some(v) = m.get("author").and_then(|v| v.as_str()) {
            tool.author = v.to_string();
        }
        if let Some(v) = m.get("license").and_then(|v| v.as_str()) {
            tool.license = Some(v.to_string());
        }
    }

    if let Some(ref existing_tool) = existing {
        tool.created_at = existing_tool.created_at;
        claw_redis::update_tool(pool, &tool)
            .await
            .map_err(|e| format!("Failed to update tool: {e}"))?;
        Ok(SyncAction::Updated)
    } else {
        claw_redis::create_tool(pool, &tool)
            .await
            .map_err(|e| format!("Failed to create tool: {e}"))?;
        Ok(SyncAction::Installed)
    }
}

/// Recursively read all text files from a directory into a HashMap.
async fn read_dir_recursive(
    base: &std::path::Path,
    dir: &std::path::Path,
    files: &mut HashMap<String, String>,
) -> Result<(), String> {
    let mut entries = tokio::fs::read_dir(dir)
        .await
        .map_err(|e| format!("Read dir failed: {e}"))?;
    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|e| format!("Read entry failed: {e}"))?
    {
        let path = entry.path();
        let file_type = entry
            .file_type()
            .await
            .map_err(|e| format!("File type failed: {e}"))?;
        if file_type.is_dir() {
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            if name == ".git" || name == "node_modules" || name == ".claw" {
                continue;
            }
            Box::pin(read_dir_recursive(base, &path, files)).await?;
        } else if file_type.is_file() {
            let rel = path
                .strip_prefix(base)
                .map_err(|e| format!("Strip prefix failed: {e}"))?;
            let rel_str = rel.to_string_lossy().to_string();
            if rel_str.starts_with('.') || rel_str.contains("/.") {
                continue;
            }
            let metadata = tokio::fs::metadata(&path)
                .await
                .map_err(|e| format!("Metadata failed: {e}"))?;
            if metadata.len() > 10 * 1024 * 1024 {
                continue; // skip files > 10MB
            }
            if let Ok(content) = tokio::fs::read_to_string(&path).await {
                files.insert(rel_str, content);
            }
        }
    }
    Ok(())
}
