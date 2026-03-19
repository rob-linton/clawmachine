use axum::{
    extract::{DefaultBodyLimit, Multipart, Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use claw_models::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::AppState;
use crate::upload_utils::{self, ExtractLimits};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/workspaces", post(create_workspace).get(list_workspaces))
        .route("/workspaces/{id}", get(get_workspace).put(update_workspace).delete(delete_workspace))
        .route("/workspaces/{id}/files", get(list_files))
        .route("/workspaces/{id}/files/{*path}", get(read_file).put(write_file).delete(delete_file))
        .route("/workspaces/{id}/upload", post(upload_zip).layer(DefaultBodyLimit::max(104_857_600)))
        .route("/workspaces/{id}/history", get(get_history))
        .route("/workspaces/{id}/revert/{hash}", post(revert_commit))
        .route("/workspaces/{id}/promote", post(promote_snapshot))
        .route("/workspaces/{id}/sync", post(sync_workspace))
        .route("/workspaces/{id}/fork", post(fork_workspace))
        .route("/workspaces/{id}/branches", get(list_branches))
        .route("/workspaces/{id}/events", get(list_events))
}

/// Enhanced workspace response with resolved derived fields.
#[derive(Serialize)]
struct WorkspaceResponse {
    #[serde(flatten)]
    workspace: Workspace,
    parent_name: Option<String>,
    child_count: u32,
    skill_names: Vec<String>,
}

/// Resolve the filesystem path for a workspace.
/// Legacy workspaces: use `ws.path` directly.
/// New workspaces: use `~/.claw/checkouts/{id}/` (for file browser) or `~/.claw/repos/{id}.git` (for bare repo).
fn resolve_workspace_dir(ws: &Workspace) -> std::path::PathBuf {
    if let Some(ref path) = ws.path {
        return path.clone();
    }
    // New-style workspace — use checkout dir for file operations
    let base = dirs::home_dir()
        .unwrap_or_else(|| "/tmp".into())
        .join(".claw")
        .join("checkouts");
    base.join(ws.id.to_string())
}

/// Get the bare repo path for a workspace.
fn bare_repo_path(ws: &Workspace) -> std::path::PathBuf {
    let base = dirs::home_dir()
        .unwrap_or_else(|| "/tmp".into())
        .join(".claw")
        .join("repos");
    base.join(format!("{}.git", ws.id))
}

/// Ensure the checkout exists (clone from bare repo if needed, pull if exists).
async fn ensure_checkout(ws: &Workspace) -> Result<std::path::PathBuf, String> {
    if ws.is_legacy() {
        let path = ws.path.as_ref().unwrap().clone();
        if !path.exists() {
            tokio::fs::create_dir_all(&path)
                .await
                .map_err(|e| format!("Failed to create workspace dir: {e}"))?;
        }
        return Ok(path);
    }

    let checkout = resolve_workspace_dir(ws);
    let repo = bare_repo_path(ws);

    if checkout.exists() {
        // Pull latest from bare repo
        let checkout_clone = checkout.clone();
        tokio::task::spawn_blocking(move || {
            std::process::Command::new("git")
                .args(["pull", "--ff-only"])
                .current_dir(&checkout_clone)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .ok();
        })
        .await
        .ok();
    } else {
        // Clone from bare repo
        let checkout_clone = checkout.clone();
        let repo_clone = repo.clone();
        tokio::task::spawn_blocking(move || {
            std::process::Command::new("git")
                .args([
                    "clone",
                    &repo_clone.to_string_lossy(),
                    &checkout_clone.to_string_lossy(),
                ])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .ok();
        })
        .await
        .ok();
    }

    Ok(checkout)
}

async fn create_workspace(
    State(state): State<AppState>,
    Json(req): Json<CreateWorkspaceRequest>,
) -> impl IntoResponse {
    match claw_redis::create_workspace(&state.pool, &req).await {
        Ok(ws) => {
            if ws.is_legacy() {
                // Legacy: create directory + git init
                let path = ws.path.as_ref().unwrap();
                if !path.exists() {
                    if let Err(e) = tokio::fs::create_dir_all(path).await {
                        tracing::warn!(error = %e, path = %path.display(), "Failed to create workspace directory");
                    }
                }
                if let Some(content) = &ws.claude_md {
                    let claude_md_path = path.join("CLAUDE.md");
                    tokio::fs::write(&claude_md_path, content).await.ok();
                }
                let ws_path = path.clone();
                tokio::task::spawn_blocking(move || {
                    init_git_repo(&ws_path);
                }).await.ok();
            } else if let Some(parent_id) = ws.parent_workspace_id {
                // Forked workspace — clone from parent's bare repo
                let parent = claw_redis::get_workspace(&state.pool, parent_id).await.ok().flatten();
                let is_snapshot = ws.persistence == WorkspacePersistence::Snapshot;

                if parent.is_none() {
                    tracing::warn!(parent_id = %parent_id, "Parent workspace not found, creating empty workspace instead");
                    let ws_id = ws.id;
                    let claude_md = ws.claude_md.clone();
                    if let Err(e) = init_bare_repo(ws_id, claude_md.as_deref(), is_snapshot).await {
                        tracing::warn!(error = %e, "Failed to init bare repo for orphaned fork");
                    }
                } else {

                // Resolve the parent ref to a commit hash
                let git_ref = ws.parent_ref.as_deref().unwrap_or("HEAD").to_string();
                let parent_repo = bare_repo_path(parent.as_ref().unwrap());
                let resolved = tokio::task::spawn_blocking({
                    let git_ref = git_ref.clone();
                    let parent_repo = parent_repo.clone();
                    move || {
                        let output = std::process::Command::new("git")
                            .args(["rev-parse", &git_ref])
                            .current_dir(&parent_repo)
                            .stdout(std::process::Stdio::piped())
                            .stderr(std::process::Stdio::piped())
                            .output();
                        match output {
                            Ok(o) if o.status.success() => Ok(String::from_utf8_lossy(&o.stdout).trim().to_string()),
                            _ => Ok(git_ref),
                        }
                    }
                }).await.unwrap_or(Ok("HEAD".into())).unwrap_or_else(|_: String| "HEAD".into());

                match fork_bare_repo(parent_id, ws.id, &resolved, is_snapshot).await {
                    Ok(()) => {
                        claw_redis::add_child_workspace(&state.pool, parent_id, ws.id).await.ok();
                        if let Some(ref p) = parent {
                            emit_event(&state.pool, parent_id, WorkspaceEventType::ChildForked, Some(&ws.id.to_string()),
                                &format!("Forked to workspace '{}'", ws.name)).await;
                        }
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "Failed to fork bare repo, falling back to init");
                        // Fall back to creating a fresh bare repo
                        if let Err(e2) = init_bare_repo(ws.id, ws.claude_md.as_deref(), is_snapshot).await {
                            tracing::warn!(error = %e2, "Fallback init_bare_repo also failed");
                        }
                    }
                }
                } // end of parent.is_some() else block
            } else {
                // New-style: create bare repo + checkout
                let ws_id = ws.id;
                let claude_md = ws.claude_md.clone();
                let is_snapshot = ws.persistence == WorkspacePersistence::Snapshot;
                if let Err(e) = init_bare_repo(ws_id, claude_md.as_deref(), is_snapshot).await {
                    tracing::warn!(error = %e, "Failed to init bare repo");
                }
            }
            // Emit initialization event
            let skill_count = ws.skill_ids.len();
            let desc = format!("Workspace created ({}, {} skills)",
                serde_json::to_string(&ws.persistence).unwrap_or_default().trim_matches('"'),
                skill_count);
            emit_event(&state.pool, ws.id, WorkspaceEventType::Initialized, None, &desc).await;

            (StatusCode::CREATED, Json(ws)).into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    }
}

#[derive(Deserialize)]
struct ListWorkspacesQuery {
    limit: Option<usize>,
    offset: Option<usize>,
}

async fn list_workspaces(
    State(state): State<AppState>,
    Query(q): Query<ListWorkspacesQuery>,
) -> impl IntoResponse {
    match claw_redis::list_workspaces(&state.pool).await {
        Ok(ws) => {
            let total = ws.len();
            let offset = q.offset.unwrap_or(0);
            let limit = q.limit.unwrap_or(50).min(100);
            let page: Vec<_> = ws.into_iter().skip(offset).take(limit).collect();
            Json(serde_json::json!({"items": page, "total": total, "offset": offset, "limit": limit})).into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    }
}

async fn get_workspace(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    match claw_redis::get_workspace(&state.pool, id).await {
        Ok(Some(ws)) => {
            // Resolve parent name
            let parent_name = if let Some(pid) = ws.parent_workspace_id {
                claw_redis::get_workspace(&state.pool, pid)
                    .await
                    .ok()
                    .flatten()
                    .map(|p| p.name)
            } else {
                None
            };

            // Resolve child count
            let child_count = claw_redis::count_children(&state.pool, id)
                .await
                .unwrap_or(0);

            // Resolve skill names
            let skill_names = resolve_skill_names(&state.pool, &ws.skill_ids).await;

            Json(WorkspaceResponse {
                workspace: ws,
                parent_name,
                child_count,
                skill_names,
            })
            .into_response()
        }
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    }
}

async fn resolve_skill_names(pool: &deadpool_redis::Pool, skill_ids: &[String]) -> Vec<String> {
    let mut names = Vec::new();
    for id in skill_ids {
        if let Ok(Some(skill)) = claw_redis::get_skill(pool, id).await {
            names.push(skill.name);
        } else {
            names.push(id.clone());
        }
    }
    names
}

async fn update_workspace(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(req): Json<CreateWorkspaceRequest>,
) -> impl IntoResponse {
    let existing = match claw_redis::get_workspace(&state.pool, id).await {
        Ok(Some(ws)) => ws,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    };

    let updated = Workspace {
        id,
        name: req.name,
        description: req.description.unwrap_or_default(),
        path: existing.path, // IMMUTABLE
        skill_ids: req.skill_ids,
        claude_md: req.claude_md,
        persistence: existing.persistence, // IMMUTABLE
        remote_url: req.remote_url.or(existing.remote_url),
        base_image: req.base_image.or(existing.base_image),
        memory_limit: req.memory_limit.or(existing.memory_limit),
        cpu_limit: req.cpu_limit.or(existing.cpu_limit),
        network_mode: req.network_mode.or(existing.network_mode),
        parent_workspace_id: existing.parent_workspace_id, // IMMUTABLE
        parent_ref: existing.parent_ref, // IMMUTABLE
        created_at: existing.created_at,
        updated_at: chrono::Utc::now(),
    };

    match claw_redis::update_workspace(&state.pool, &updated).await {
        Ok(()) => Json(updated).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    }
}

#[derive(Deserialize)]
struct DeleteQuery {
    #[serde(default)]
    delete_files: bool,
}

async fn delete_workspace(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Query(query): Query<DeleteQuery>,
) -> impl IntoResponse {
    let ws = match claw_redis::get_workspace(&state.pool, id).await {
        Ok(Some(ws)) => ws,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    };

    match claw_redis::delete_workspace(&state.pool, id).await {
        Ok(()) => {
            if query.delete_files {
                if ws.is_legacy() {
                    if let Some(ref path) = ws.path {
                        tokio::fs::remove_dir_all(path).await.ok();
                    }
                } else {
                    // Clean up bare repo + checkout
                    let repo = bare_repo_path(&ws);
                    let checkout = resolve_workspace_dir(&ws);
                    tokio::fs::remove_dir_all(&repo).await.ok();
                    tokio::fs::remove_dir_all(&checkout).await.ok();
                }
            }
            StatusCode::NO_CONTENT.into_response()
        }
        Err(e) => {
            let msg = e.to_string();
            if msg.contains("referenced by") || msg.contains("active jobs") {
                (StatusCode::CONFLICT, Json(serde_json::json!({"error": msg}))).into_response()
            } else {
                (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": msg}))).into_response()
            }
        }
    }
}

// --- File browser endpoints ---

/// Validate that a resolved path is within the workspace directory (prevent path traversal).
fn validate_path(workspace_path: &std::path::Path, requested: &str) -> Result<std::path::PathBuf, StatusCode> {
    if requested.contains("..") {
        return Err(StatusCode::FORBIDDEN);
    }
    let resolved = workspace_path.join(requested);
    let ws_canonical = workspace_path.canonicalize().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let resolved_canonical = resolved.canonicalize().map_err(|_| StatusCode::NOT_FOUND)?;
    if !resolved_canonical.starts_with(&ws_canonical) {
        return Err(StatusCode::FORBIDDEN);
    }
    Ok(resolved_canonical)
}

async fn list_files(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    let ws = match claw_redis::get_workspace(&state.pool, id).await {
        Ok(Some(ws)) => ws,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    };

    let ws_dir = match ensure_checkout(&ws).await {
        Ok(d) => d,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e}))).into_response(),
    };

    if !ws_dir.exists() {
        return Json(serde_json::json!({"files": []})).into_response();
    }
    match list_dir_entries(&ws_dir, 10, 2000).await {
        Ok(files) => Json(serde_json::json!({"files": files})).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    }
}

async fn list_dir_entries(
    base: &std::path::Path,
    max_depth: u32,
    max_entries: usize,
) -> Result<Vec<serde_json::Value>, std::io::Error> {
    let mut entries = Vec::new();
    let mut stack: Vec<(std::path::PathBuf, u32)> = vec![(base.to_path_buf(), 0)];

    while let Some((dir, depth)) = stack.pop() {
        if depth > max_depth || entries.len() >= max_entries {
            break;
        }
        let mut read_dir = tokio::fs::read_dir(&dir).await?;
        while let Some(entry) = read_dir.next_entry().await? {
            if entries.len() >= max_entries {
                break;
            }
            let path = entry.path();
            if entry.file_name() == ".git" {
                continue;
            }
            let relative = path.strip_prefix(base).unwrap_or(&path);
            let is_dir = path.is_dir();
            let size = if is_dir { 0 } else { entry.metadata().await.map(|m| m.len()).unwrap_or(0) };
            entries.push(serde_json::json!({
                "path": relative.to_string_lossy(),
                "is_dir": is_dir,
                "size": size,
            }));
            if is_dir && depth < max_depth {
                stack.push((path, depth + 1));
            }
        }
    }
    Ok(entries)
}

async fn read_file(
    State(state): State<AppState>,
    Path((id, file_path)): Path<(Uuid, String)>,
) -> impl IntoResponse {
    let ws = match claw_redis::get_workspace(&state.pool, id).await {
        Ok(Some(ws)) => ws,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    };

    let ws_dir = match ensure_checkout(&ws).await {
        Ok(d) => d,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e}))).into_response(),
    };

    let resolved = match validate_path(&ws_dir, &file_path) {
        Ok(p) => p,
        Err(status) => return status.into_response(),
    };

    match tokio::fs::read_to_string(&resolved).await {
        Ok(content) => Json(serde_json::json!({
            "path": file_path,
            "content": content,
        })).into_response(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => StatusCode::NOT_FOUND.into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    }
}

#[derive(Deserialize)]
struct WriteFileRequest {
    content: String,
}

async fn write_file(
    State(state): State<AppState>,
    Path((id, file_path)): Path<(Uuid, String)>,
    Json(req): Json<WriteFileRequest>,
) -> impl IntoResponse {
    let ws = match claw_redis::get_workspace(&state.pool, id).await {
        Ok(Some(ws)) => ws,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    };

    let ws_dir = match ensure_checkout(&ws).await {
        Ok(d) => d,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e}))).into_response(),
    };

    if file_path.contains("..") {
        return StatusCode::FORBIDDEN.into_response();
    }
    let resolved = ws_dir.join(&file_path);
    if let Some(parent) = resolved.parent() {
        if !parent.exists() {
            if let Err(e) = tokio::fs::create_dir_all(parent).await {
                return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response();
            }
        }
        if let Ok(parent_canonical) = parent.canonicalize() {
            if let Ok(ws_canonical) = ws_dir.canonicalize() {
                if !parent_canonical.starts_with(&ws_canonical) {
                    return StatusCode::FORBIDDEN.into_response();
                }
            }
        }
    }

    match tokio::fs::write(&resolved, &req.content).await {
        Ok(()) => {
            emit_event(&state.pool, id, WorkspaceEventType::FileModified, None,
                &format!("File written: {}", file_path)).await;
            StatusCode::NO_CONTENT.into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    }
}

async fn delete_file(
    State(state): State<AppState>,
    Path((id, file_path)): Path<(Uuid, String)>,
) -> impl IntoResponse {
    if file_path.trim_matches('/').is_empty() {
        return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": "Path cannot be empty"}))).into_response();
    }

    let ws = match claw_redis::get_workspace(&state.pool, id).await {
        Ok(Some(ws)) => ws,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    };

    let ws_dir = match ensure_checkout(&ws).await {
        Ok(d) => d,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e}))).into_response(),
    };

    let resolved = match validate_path(&ws_dir, &file_path) {
        Ok(p) => p,
        Err(status) => return status.into_response(),
    };

    if resolved.is_dir() {
        match tokio::fs::remove_dir_all(&resolved).await {
            Ok(()) => {
                emit_event(&state.pool, id, WorkspaceEventType::FileModified, None,
                    &format!("Folder deleted: {}", file_path)).await;
                StatusCode::NO_CONTENT.into_response()
            }
            Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
        }
    } else {
        match tokio::fs::remove_file(&resolved).await {
            Ok(()) => {
                emit_event(&state.pool, id, WorkspaceEventType::FileModified, None,
                    &format!("File deleted: {}", file_path)).await;
                StatusCode::NO_CONTENT.into_response()
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => StatusCode::NOT_FOUND.into_response(),
            Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
        }
    }
}

// --- ZIP Upload ---

async fn upload_zip(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    mut multipart: Multipart,
) -> impl IntoResponse {
    let ws = match claw_redis::get_workspace(&state.pool, id).await {
        Ok(Some(ws)) => ws,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    };

    let ws_dir = match ensure_checkout(&ws).await {
        Ok(d) => d,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e}))).into_response(),
    };

    if !ws_dir.exists() {
        if let Err(e) = tokio::fs::create_dir_all(&ws_dir).await {
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": format!("Failed to create workspace dir: {e}")}))).into_response();
        }
    }

    let mut zip_data: Option<Vec<u8>> = None;
    let mut prefix = String::new();

    while let Ok(Some(field)) = multipart.next_field().await {
        let name = field.name().unwrap_or("").to_string();
        match name.as_str() {
            "file" => {
                match field.bytes().await {
                    Ok(bytes) => zip_data = Some(bytes.to_vec()),
                    Err(e) => return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": format!("Failed to read file: {e}")}))).into_response(),
                }
            }
            "path" => {
                if let Ok(text) = field.text().await {
                    if text.contains("..") || text.starts_with('/') {
                        return (StatusCode::FORBIDDEN, Json(serde_json::json!({"error": "Invalid path prefix"}))).into_response();
                    }
                    prefix = text;
                }
            }
            _ => {}
        }
    }

    let Some(data) = zip_data else {
        return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": "No file field in upload"}))).into_response();
    };

    let limits = ExtractLimits {
        max_total_size: 500 * 1024 * 1024,
        ..Default::default()
    };

    match upload_utils::extract_zip_to_dir(&data, &ws_dir, &prefix, &limits).await {
        Ok(result) => {
            tracing::info!(workspace_id = %id, uploaded = result.uploaded, skipped = result.skipped, "ZIP uploaded to workspace");
            Json(result).into_response()
        }
        Err(e) => (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": e}))).into_response(),
    }
}

// --- Git operations ---

/// Initialize a legacy workspace git repo (direct directory).
fn init_git_repo(path: &std::path::Path) {
    use std::process::Command;

    if path.join(".git").exists() {
        return;
    }

    let gitignore = path.join(".gitignore");
    std::fs::write(&gitignore, ".claw/\n.DS_Store\nnode_modules/\n__pycache__/\ntarget/\n.env*\n").ok();

    let run = |args: &[&str]| -> bool {
        Command::new("git")
            .args(args)
            .current_dir(path)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    };

    if run(&["init"]) {
        run(&["add", "-A"]);
        run(&["-c", "user.name=ClaudeCodeClaw", "-c", "user.email=claw@local", "commit", "-m", "claw: workspace initialized", "--allow-empty"]);
    }
}

/// Initialize a new-style bare repo + checkout.
/// 1. git init --bare repos/{id}.git
/// 2. git clone repos/{id}.git checkouts/{id}/
/// 3. Create .gitignore + CLAUDE.md in checkout
/// 4. git add + commit + push
/// 5. For snapshot: tag claw/base
async fn init_bare_repo(ws_id: Uuid, claude_md: Option<&str>, is_snapshot: bool) -> Result<(), String> {
    let home = dirs::home_dir().unwrap_or_else(|| "/tmp".into());
    let repos_dir = home.join(".claw").join("repos");
    let checkouts_dir = home.join(".claw").join("checkouts");
    let repo_path = repos_dir.join(format!("{}.git", ws_id));
    let checkout_path = checkouts_dir.join(ws_id.to_string());

    // Ensure parent dirs exist
    tokio::fs::create_dir_all(&repos_dir)
        .await
        .map_err(|e| format!("Failed to create repos dir: {e}"))?;
    tokio::fs::create_dir_all(&checkouts_dir)
        .await
        .map_err(|e: std::io::Error| format!("Failed to create checkouts dir: {e}"))?;

    let repo_p = repo_path.clone();
    let checkout_p = checkout_path.clone();
    let claude_md_owned = claude_md.map(|s| s.to_string());

    tokio::task::spawn_blocking(move || {
        use std::process::Command;

        let run = |cmd: &str, args: &[&str], dir: &std::path::Path| -> Result<(), String> {
            let output = Command::new(cmd)
                .args(args)
                .current_dir(dir)
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .output()
                .map_err(|e| format!("Failed to run {cmd}: {e}"))?;
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(format!("{cmd} failed: {stderr}"));
            }
            Ok(())
        };

        // 1. git init --bare
        std::fs::create_dir_all(&repo_p).map_err(|e| format!("mkdir: {e}"))?;
        run("git", &["init", "--bare"], &repo_p)?;

        // 2. git clone bare repo → checkout
        run("git", &["clone", &repo_p.to_string_lossy(), &checkout_p.to_string_lossy()], &repos_dir)?;

        // 3. Create .gitignore
        let gitignore_content = ".claw/\n.DS_Store\nnode_modules/\n__pycache__/\ntarget/\n.env*\n";
        std::fs::write(checkout_p.join(".gitignore"), gitignore_content)
            .map_err(|e| format!("write .gitignore: {e}"))?;

        // Write CLAUDE.md if provided
        if let Some(ref content) = claude_md_owned {
            std::fs::write(checkout_p.join("CLAUDE.md"), content)
                .map_err(|e| format!("write CLAUDE.md: {e}"))?;
        }

        // 4. git add + commit + push
        run("git", &["add", "-A"], &checkout_p)?;
        run("git", &[
            "-c", "user.name=ClaudeCodeClaw",
            "-c", "user.email=claw@local",
            "commit", "-m", "claw: workspace initialized",
        ], &checkout_p)?;
        run("git", &["push", "origin", "HEAD"], &checkout_p)?;

        // 5. Tag for snapshot mode
        if is_snapshot {
            run("git", &["tag", "claw/base"], &repo_p)?;
        }

        Ok(())
    })
    .await
    .map_err(|e| format!("spawn_blocking: {e}"))?
}

async fn get_history(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    let ws = match claw_redis::get_workspace(&state.pool, id).await {
        Ok(Some(ws)) => ws,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    };

    let ws_dir = match ensure_checkout(&ws).await {
        Ok(d) => d,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e}))).into_response(),
    };

    let result = tokio::task::spawn_blocking(move || {
        let output = std::process::Command::new("git")
            .args(["log", "--oneline", "--format=%H|%s|%aI", "-20"])
            .current_dir(&ws_dir)
            .output();

        match output {
            Ok(o) if o.status.success() => {
                let stdout = String::from_utf8_lossy(&o.stdout);
                let commits: Vec<serde_json::Value> = stdout
                    .lines()
                    .filter(|l| !l.is_empty())
                    .map(|line| {
                        let parts: Vec<&str> = line.splitn(3, '|').collect();
                        serde_json::json!({
                            "hash": parts.first().unwrap_or(&""),
                            "message": parts.get(1).unwrap_or(&""),
                            "date": parts.get(2).unwrap_or(&""),
                        })
                    })
                    .collect();
                Ok(commits)
            }
            Ok(o) => Err(String::from_utf8_lossy(&o.stderr).to_string()),
            Err(e) => Err(e.to_string()),
        }
    }).await.unwrap_or(Err("Task failed".into()));

    match result {
        Ok(commits) => Json(serde_json::json!({"commits": commits})).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e}))).into_response(),
    }
}

async fn revert_commit(
    State(state): State<AppState>,
    Path((id, hash)): Path<(Uuid, String)>,
) -> impl IntoResponse {
    let ws = match claw_redis::get_workspace(&state.pool, id).await {
        Ok(Some(ws)) => ws,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    };

    if !hash.chars().all(|c| c.is_ascii_hexdigit()) || hash.len() < 7 || hash.len() > 40 {
        return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": "Invalid commit hash"}))).into_response();
    }

    let ws_dir = match ensure_checkout(&ws).await {
        Ok(d) => d,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e}))).into_response(),
    };

    let hash_clone = hash.clone();
    let result = tokio::task::spawn_blocking(move || {
        let output = std::process::Command::new("git")
            .args(["-c", "user.name=ClaudeCodeClaw", "-c", "user.email=claw@local", "revert", "--no-edit", &hash_clone])
            .current_dir(&ws_dir)
            .output();

        match output {
            Ok(o) if o.status.success() => Ok(()),
            Ok(o) => {
                std::process::Command::new("git")
                    .args(["revert", "--abort"])
                    .current_dir(&ws_dir)
                    .output()
                    .ok();
                Err(String::from_utf8_lossy(&o.stderr).to_string())
            }
            Err(e) => Err(e.to_string()),
        }
    }).await.unwrap_or(Err("Task failed".into()));

    match result {
        Ok(()) => {
            let short = if hash.len() >= 7 { &hash[..7] } else { &hash };
            emit_event(&state.pool, id, WorkspaceEventType::Reverted, Some(&hash),
                &format!("Reverted commit {}", short)).await;
            Json(serde_json::json!({"reverted": hash})).into_response()
        }
        Err(e) => (StatusCode::CONFLICT, Json(serde_json::json!({"error": format!("Revert failed: {e}")}))).into_response(),
    }
}

// --- Snapshot promote ---

#[derive(Deserialize)]
struct PromoteQuery {
    #[serde(rename = "ref")]
    git_ref: String,
}

/// Move the claw/base tag to a specific commit/branch ref (for snapshot workspaces).
async fn promote_snapshot(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Query(query): Query<PromoteQuery>,
) -> impl IntoResponse {
    let ws = match claw_redis::get_workspace(&state.pool, id).await {
        Ok(Some(ws)) => ws,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    };

    if ws.persistence != WorkspacePersistence::Snapshot {
        return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": "Promote is only for snapshot workspaces"}))).into_response();
    }

    if ws.is_legacy() {
        return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": "Promote not supported for legacy workspaces"}))).into_response();
    }

    let repo = bare_repo_path(&ws);
    let git_ref = query.git_ref.clone();

    let result = tokio::task::spawn_blocking(move || {
        use std::process::Command;
        // Delete old tag and create new one pointing at the given ref
        Command::new("git")
            .args(["tag", "-d", "claw/base"])
            .current_dir(&repo)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .ok(); // May fail if tag doesn't exist yet — that's fine

        let output = Command::new("git")
            .args(["tag", "claw/base", &git_ref])
            .current_dir(&repo)
            .output();

        match output {
            Ok(o) if o.status.success() => Ok(()),
            Ok(o) => Err(String::from_utf8_lossy(&o.stderr).to_string()),
            Err(e) => Err(e.to_string()),
        }
    }).await.unwrap_or(Err("Task failed".into()));

    match result {
        Ok(()) => {
            emit_event(&state.pool, id, WorkspaceEventType::SnapshotPromoted, Some(&query.git_ref),
                &format!("Promoted {} to claw/base", query.git_ref)).await;
            Json(serde_json::json!({"promoted": query.git_ref, "tag": "claw/base"})).into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": format!("Promote failed: {e}")}))).into_response(),
    }
}

// --- Remote sync ---

/// Pull latest changes from a workspace's remote URL into the local bare repo and checkout.
async fn sync_workspace(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    let ws = match claw_redis::get_workspace(&state.pool, id).await {
        Ok(Some(ws)) => ws,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    };

    if ws.is_legacy() {
        return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": "Sync not supported for legacy workspaces"}))).into_response();
    }

    let remote_url = match &ws.remote_url {
        Some(url) if !url.is_empty() => url.clone(),
        _ => return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": "Workspace has no remote URL configured"}))).into_response(),
    };

    let repo = bare_repo_path(&ws);
    let checkout = resolve_workspace_dir(&ws);

    let result: Result<(), String> = tokio::task::spawn_blocking(move || {
        use std::process::Command;

        let run = |args: &[&str], dir: &std::path::Path| -> Result<(), String> {
            let output = Command::new("git")
                .args(args)
                .current_dir(dir)
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .output()
                .map_err(|e| format!("git failed: {e}"))?;
            if !output.status.success() {
                return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
            }
            Ok(())
        };

        // Add/update remote in bare repo
        let _ = run(&["remote", "add", "upstream", &remote_url], &repo);
        let _ = run(&["remote", "set-url", "upstream", &remote_url], &repo);

        // Fetch from upstream
        run(&["fetch", "upstream"], &repo)?;

        // Update main branch ref to match upstream (force to handle diverged history)
        run(&["fetch", "upstream", "+HEAD:refs/heads/main"], &repo)?;

        // Update checkout if it exists (force reset to match bare repo)
        if checkout.exists() {
            run(&["fetch", "origin"], &checkout)?;
            run(&["reset", "--hard", "origin/main"], &checkout)?;
        }

        Ok(())
    }).await.unwrap_or(Err("Task failed".into()));

    match result {
        Ok(()) => {
            emit_event(&state.pool, id, WorkspaceEventType::Synced, None,
                &format!("Synced from {}", ws.remote_url.as_deref().unwrap_or("remote"))).await;
            Json(serde_json::json!({"synced": true, "remote_url": ws.remote_url})).into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": format!("Sync failed: {e}")}))).into_response(),
    }
}

// --- Fork workspace ---

#[derive(Deserialize)]
struct ForkRequest {
    name: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    persistence: Option<WorkspacePersistence>,
    /// Git ref to fork from (defaults to HEAD).
    #[serde(default, rename = "ref")]
    git_ref: Option<String>,
    #[serde(default)]
    skill_ids: Option<Vec<String>>,
    #[serde(default)]
    claude_md: Option<Option<String>>,
    #[serde(default)]
    network_mode: Option<String>,
    #[serde(default)]
    memory_limit: Option<String>,
    #[serde(default)]
    cpu_limit: Option<f64>,
    #[serde(default)]
    base_image: Option<String>,
}

async fn fork_workspace(
    State(state): State<AppState>,
    Path(parent_id): Path<Uuid>,
    Json(req): Json<ForkRequest>,
) -> impl IntoResponse {
    let parent = match claw_redis::get_workspace(&state.pool, parent_id).await {
        Ok(Some(ws)) => ws,
        Ok(None) => return (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "Parent workspace not found"}))).into_response(),
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    };

    if parent.is_legacy() {
        return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": "Cannot fork legacy workspaces (no bare repo)"}))).into_response();
    }

    // Resolve the git ref to a commit hash
    let git_ref = req.git_ref.as_deref().unwrap_or("HEAD");
    let parent_repo = bare_repo_path(&parent);

    let ref_to_resolve = git_ref.to_string();
    let parent_repo_clone = parent_repo.clone();
    let resolved_ref = match tokio::task::spawn_blocking(move || {
        let output = std::process::Command::new("git")
            .args(["rev-parse", &ref_to_resolve])
            .current_dir(&parent_repo_clone)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .output();
        match output {
            Ok(o) if o.status.success() => Ok(String::from_utf8_lossy(&o.stdout).trim().to_string()),
            Ok(o) => Err(format!("Invalid ref '{}': {}", ref_to_resolve, String::from_utf8_lossy(&o.stderr).trim())),
            Err(e) => Err(format!("git rev-parse failed: {e}")),
        }
    }).await {
        Ok(Ok(hash)) => hash,
        Ok(Err(e)) => return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": e}))).into_response(),
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": format!("Task failed: {e}")}))).into_response(),
    };

    // Create the workspace in Redis
    let persistence = req.persistence.unwrap_or_else(|| parent.persistence.clone());
    let is_snapshot = persistence == WorkspacePersistence::Snapshot;
    let create_req = CreateWorkspaceRequest {
        name: req.name,
        description: req.description,
        path: None,
        skill_ids: req.skill_ids.unwrap_or_else(|| parent.skill_ids.clone()),
        claude_md: req.claude_md.unwrap_or_else(|| parent.claude_md.clone()),
        persistence: Some(persistence),
        remote_url: None, // Don't inherit remote_url
        base_image: req.base_image.or_else(|| parent.base_image.clone()),
        memory_limit: req.memory_limit.or_else(|| parent.memory_limit.clone()),
        cpu_limit: req.cpu_limit.or(parent.cpu_limit),
        network_mode: req.network_mode.or_else(|| parent.network_mode.clone()),
        parent_workspace_id: Some(parent_id),
        parent_ref: Some(if git_ref == "HEAD" { resolved_ref.clone() } else { git_ref.to_string() }),
    };

    let new_ws = match claw_redis::create_workspace(&state.pool, &create_req).await {
        Ok(ws) => ws,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    };

    let new_id = new_ws.id;

    // Fork the bare repo
    match fork_bare_repo(parent_id, new_id, &resolved_ref, is_snapshot).await {
        Ok(()) => {
            // Record parent-child relationship
            claw_redis::add_child_workspace(&state.pool, parent_id, new_id).await.ok();

            // Emit events
            emit_event(&state.pool, new_id, WorkspaceEventType::Forked, Some(&parent_id.to_string()),
                &format!("Forked from workspace '{}' at {}", parent.name, &resolved_ref[..7.min(resolved_ref.len())])).await;
            emit_event(&state.pool, parent_id, WorkspaceEventType::ChildForked, Some(&new_id.to_string()),
                &format!("Forked to workspace '{}'", new_ws.name)).await;

            (StatusCode::CREATED, Json(new_ws)).into_response()
        }
        Err(e) => {
            // Rollback: delete the Redis entry
            tracing::error!(error = %e, new_id = %new_id, "Fork failed, rolling back");
            claw_redis::delete_workspace(&state.pool, new_id).await.ok();
            (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": format!("Fork failed: {e}")}))).into_response()
        }
    }
}

/// Clone a bare repo and set up the fork.
async fn fork_bare_repo(parent_id: Uuid, new_id: Uuid, commit_hash: &str, is_snapshot: bool) -> Result<(), String> {
    let home = dirs::home_dir().unwrap_or_else(|| "/tmp".into());
    let repos_dir = home.join(".claw").join("repos");
    let checkouts_dir = home.join(".claw").join("checkouts");
    let parent_repo = repos_dir.join(format!("{}.git", parent_id));
    let new_repo = repos_dir.join(format!("{}.git", new_id));
    let new_checkout = checkouts_dir.join(new_id.to_string());

    let commit = commit_hash.to_string();

    tokio::task::spawn_blocking(move || {
        use std::process::Command;

        let run = |cmd: &str, args: &[&str], dir: &std::path::Path| -> Result<(), String> {
            let output = Command::new(cmd)
                .args(args)
                .current_dir(dir)
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .output()
                .map_err(|e| format!("Failed to run {cmd}: {e}"))?;
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(format!("{cmd} failed: {stderr}"));
            }
            Ok(())
        };

        // 1. Clone bare repo
        run("git", &["clone", "--bare", &parent_repo.to_string_lossy(), &new_repo.to_string_lossy()], &repos_dir)?;

        // 2. Reset main to the target commit
        run("git", &["symbolic-ref", "HEAD", "refs/heads/main"], &new_repo)?;
        run("git", &["update-ref", "refs/heads/main", &commit], &new_repo)?;

        // 3. Clean up inherited branches
        let branch_output = Command::new("git")
            .args(["for-each-ref", "--format=%(refname:short)", "refs/heads/"])
            .current_dir(&new_repo)
            .stdout(std::process::Stdio::piped())
            .output()
            .map_err(|e| format!("list branches: {e}"))?;
        let branches = String::from_utf8_lossy(&branch_output.stdout);
        for branch in branches.lines() {
            let branch = branch.trim();
            if branch != "main" && !branch.is_empty() {
                Command::new("git")
                    .args(["branch", "-D", branch])
                    .current_dir(&new_repo)
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status()
                    .ok();
            }
        }

        // 4. Remove origin remote
        Command::new("git")
            .args(["remote", "remove", "origin"])
            .current_dir(&new_repo)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .ok();

        // 5. Clone into checkout
        std::fs::create_dir_all(&checkouts_dir).map_err(|e| format!("mkdir checkouts: {e}"))?;
        run("git", &["clone", &new_repo.to_string_lossy(), &new_checkout.to_string_lossy()], &checkouts_dir)?;

        // 6. Tag for snapshot mode
        if is_snapshot {
            run("git", &["tag", "claw/base"], &new_repo)?;
        }

        Ok(())
    })
    .await
    .map_err(|e| format!("spawn_blocking: {e}"))?
}

// --- List branches ---

async fn list_branches(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    let ws = match claw_redis::get_workspace(&state.pool, id).await {
        Ok(Some(ws)) => ws,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    };

    if ws.is_legacy() {
        return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": "Branches not available for legacy workspaces"}))).into_response();
    }

    let repo = bare_repo_path(&ws);

    let result = tokio::task::spawn_blocking(move || {
        let output = std::process::Command::new("git")
            .args(["for-each-ref", "--format=%(refname:short)|%(objectname:short)|%(creatordate:iso)", "refs/heads/"])
            .current_dir(&repo)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .output()
            .map_err(|e| format!("git for-each-ref failed: {e}"))?;

        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let branches: Vec<serde_json::Value> = stdout
            .lines()
            .filter_map(|line| {
                let parts: Vec<&str> = line.splitn(3, '|').collect();
                if parts.len() >= 2 {
                    let name = parts[0].trim();
                    let hash = parts[1].trim();
                    let date = parts.get(2).map(|d| d.trim()).unwrap_or("");
                    // Extract job_id from snapshot branch names
                    let job_id = name.strip_prefix("claw/snapshot-").map(|s| s.to_string());
                    Some(serde_json::json!({
                        "name": name,
                        "hash": hash,
                        "date": date,
                        "job_id": job_id,
                    }))
                } else {
                    None
                }
            })
            .collect();

        Ok(branches)
    })
    .await
    .unwrap_or(Err("Task failed".into()));

    match result {
        Ok(branches) => Json(serde_json::json!({"branches": branches})).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e}))).into_response(),
    }
}

// --- Workspace events timeline ---

#[derive(Deserialize)]
struct EventsQuery {
    limit: Option<usize>,
    offset: Option<usize>,
}

async fn list_events(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Query(query): Query<EventsQuery>,
) -> impl IntoResponse {
    let limit = query.limit.unwrap_or(50).min(100);
    let offset = query.offset.unwrap_or(0);

    match claw_redis::list_workspace_events(&state.pool, id, limit, offset).await {
        Ok((events, total)) => Json(serde_json::json!({
            "events": events,
            "total": total,
            "limit": limit,
            "offset": offset,
        })).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    }
}

// --- Event emission helper ---

async fn emit_event(
    pool: &deadpool_redis::Pool,
    workspace_id: Uuid,
    event_type: WorkspaceEventType,
    related_id: Option<&str>,
    description: &str,
) {
    let event = WorkspaceEvent {
        timestamp: chrono::Utc::now(),
        event_type,
        related_id: related_id.map(|s| s.to_string()),
        description: description.to_string(),
    };
    if let Err(e) = claw_redis::append_workspace_event(pool, workspace_id, &event).await {
        tracing::warn!(error = %e, workspace_id = %workspace_id, "Failed to emit workspace event");
    }
}
