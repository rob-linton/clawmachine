use axum::{
    extract::{DefaultBodyLimit, Multipart, Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use claw_models::*;
use serde::Deserialize;
use uuid::Uuid;

use crate::AppState;
use crate::upload_utils::{self, ExtractLimits};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/workspaces", post(create_workspace).get(list_workspaces))
        .route("/workspaces/{id}", get(get_workspace).put(update_workspace).delete(delete_workspace))
        .route("/workspaces/{id}/files", get(list_files))
        .route("/workspaces/{id}/files/{*path}", get(read_file).put(write_file))
        .route("/workspaces/{id}/upload", post(upload_zip).layer(DefaultBodyLimit::max(104_857_600)))
}

async fn create_workspace(
    State(state): State<AppState>,
    Json(req): Json<CreateWorkspaceRequest>,
) -> impl IntoResponse {
    match claw_redis::create_workspace(&state.pool, &req).await {
        Ok(ws) => {
            // Create directory on disk if it doesn't exist
            if !ws.path.exists() {
                if let Err(e) = tokio::fs::create_dir_all(&ws.path).await {
                    tracing::warn!(error = %e, path = %ws.path.display(), "Failed to create workspace directory");
                }
            }
            // Write CLAUDE.md if provided
            if let Some(content) = &ws.claude_md {
                let claude_md_path = ws.path.join("CLAUDE.md");
                tokio::fs::write(&claude_md_path, content).await.ok();
            }
            (StatusCode::CREATED, Json(ws)).into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    }
}

async fn list_workspaces(State(state): State<AppState>) -> impl IntoResponse {
    match claw_redis::list_workspaces(&state.pool).await {
        Ok(ws) => Json(serde_json::json!({"items": ws, "total": ws.len()})).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    }
}

async fn get_workspace(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    match claw_redis::get_workspace(&state.pool, id).await {
        Ok(Some(ws)) => Json(ws).into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    }
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
        path: existing.path, // Path cannot be changed after creation
        skill_ids: req.skill_ids,
        claude_md: req.claude_md,
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
    // Get workspace path before deleting
    let ws = match claw_redis::get_workspace(&state.pool, id).await {
        Ok(Some(ws)) => ws,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    };

    match claw_redis::delete_workspace(&state.pool, id).await {
        Ok(()) => {
            if query.delete_files {
                tokio::fs::remove_dir_all(&ws.path).await.ok();
            }
            StatusCode::NO_CONTENT.into_response()
        }
        Err(e) => {
            let msg = e.to_string();
            if msg.contains("referenced by cron") {
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
    // Early reject any path containing ..
    if requested.contains("..") {
        return Err(StatusCode::FORBIDDEN);
    }

    let resolved = workspace_path.join(requested);

    // Canonicalize both paths for comparison (workspace path may not be canonical)
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

    if !ws.path.exists() {
        return Json(serde_json::json!({"files": []})).into_response();
    }
    match list_dir_entries(&ws.path, 3, 500).await {
        Ok(files) => Json(serde_json::json!({"files": files})).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    }
}

/// List files iteratively using a stack (avoids async recursion).
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

    let resolved = match validate_path(&ws.path, &file_path) {
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

    // Validate path (can't canonicalize non-existent file, so check parent)
    if file_path.contains("..") {
        return StatusCode::FORBIDDEN.into_response();
    }
    let resolved = ws.path.join(&file_path);
    if let Some(parent) = resolved.parent() {
        if !parent.exists() {
            if let Err(e) = tokio::fs::create_dir_all(parent).await {
                return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response();
            }
        }
        // Verify parent is within workspace
        if let Ok(parent_canonical) = parent.canonicalize() {
            if let Ok(ws_canonical) = ws.path.canonicalize() {
                if !parent_canonical.starts_with(&ws_canonical) {
                    return StatusCode::FORBIDDEN.into_response();
                }
            }
        }
    }

    match tokio::fs::write(&resolved, &req.content).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
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

    // Ensure workspace directory exists
    if !ws.path.exists() {
        if let Err(e) = tokio::fs::create_dir_all(&ws.path).await {
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

    match upload_utils::extract_zip_to_dir(&data, &ws.path, &prefix, &limits).await {
        Ok(result) => {
            tracing::info!(workspace_id = %id, uploaded = result.uploaded, skipped = result.skipped, "ZIP uploaded to workspace");
            Json(result).into_response()
        }
        Err(e) => (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": e}))).into_response(),
    }
}
