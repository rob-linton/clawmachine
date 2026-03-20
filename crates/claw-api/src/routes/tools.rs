use axum::{
    extract::{DefaultBodyLimit, Multipart, Path, State},
    http::{header, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use std::io::Write;

use crate::AppState;
use crate::upload_utils::{self, ExtractLimits};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/tools", post(create_tool).get(list_tools))
        .route("/tools/upload", post(upload_tool_zip).layer(DefaultBodyLimit::max(104_857_600)))
        .route("/tools/{id}", get(get_tool).put(update_tool).delete(delete_tool))
        .route("/tools/{id}/download", get(download_tool))
}

async fn create_tool(
    State(state): State<AppState>,
    Json(req): Json<claw_models::CreateToolRequest>,
) -> impl IntoResponse {
    let tool = claw_redis::new_tool(&req.id, &req.name, &req.install_commands, &req.check_command);
    let tool = claw_models::Tool {
        description: req.description,
        tags: req.tags,
        env_vars: req.env_vars,
        auth_script: req.auth_script,
        ..tool
    };
    match claw_redis::create_tool(&state.pool, &tool).await {
        Ok(()) => (StatusCode::CREATED, Json(tool)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    }
}

async fn list_tools(State(state): State<AppState>) -> impl IntoResponse {
    match claw_redis::list_tools(&state.pool).await {
        Ok(tools) => Json(serde_json::json!({"items": tools, "total": tools.len()})).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    }
}

async fn get_tool(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match claw_redis::get_tool(&state.pool, &id).await {
        Ok(Some(tool)) => Json(tool).into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    }
}

async fn update_tool(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<claw_models::CreateToolRequest>,
) -> impl IntoResponse {
    let mut tool = claw_redis::new_tool(&id, &req.name, &req.install_commands, &req.check_command);
    tool.description = req.description;
    tool.tags = req.tags;
    tool.env_vars = req.env_vars;
    tool.auth_script = req.auth_script;

    if let Ok(Some(existing)) = claw_redis::get_tool(&state.pool, &id).await {
        tool.created_at = existing.created_at;
    }
    match claw_redis::update_tool(&state.pool, &tool).await {
        Ok(()) => Json(tool).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    }
}

async fn delete_tool(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match claw_redis::delete_tool(&state.pool, &id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    }
}

// --- ZIP Download ---

async fn download_tool(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let tool = match claw_redis::get_tool(&state.pool, &id).await {
        Ok(Some(t)) => t,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    };

    let buf = std::io::Cursor::new(Vec::new());
    let mut zip = zip::ZipWriter::new(buf);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    // Write TOOL.json
    let tool_json = serde_json::json!({
        "install_commands": tool.install_commands,
        "check_command": tool.check_command,
        "env_vars": tool.env_vars,
        "auth_script": tool.auth_script,
    });
    if let Err(e) = zip.start_file("TOOL.json", options) {
        return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": format!("zip error: {e}")}))).into_response();
    }
    let tool_bytes = serde_json::to_vec_pretty(&tool_json).unwrap_or_default();
    if let Err(e) = zip.write_all(&tool_bytes) {
        return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": format!("zip write error: {e}")}))).into_response();
    }

    // Write manifest.json
    let manifest = serde_json::json!({
        "format": "claw-tool-v1",
        "id": tool.id,
        "name": tool.name,
        "description": tool.description,
        "tags": tool.tags,
        "version": tool.version,
        "author": tool.author,
        "license": tool.license,
    });
    if let Err(e) = zip.start_file("manifest.json", options) {
        return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": format!("zip error: {e}")}))).into_response();
    }
    let manifest_bytes = serde_json::to_vec_pretty(&manifest).unwrap_or_default();
    if let Err(e) = zip.write_all(&manifest_bytes) {
        return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": format!("zip write error: {e}")}))).into_response();
    }

    let cursor = match zip.finish() {
        Ok(c) => c,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": format!("zip finish error: {e}")}))).into_response(),
    };

    let bytes = cursor.into_inner();
    let filename = format!("{}.zip", tool.id);

    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "application/zip".to_string()),
            (header::CONTENT_DISPOSITION, format!("attachment; filename=\"{filename}\"")),
        ],
        bytes,
    ).into_response()
}

// --- ZIP Upload ---

async fn upload_tool_zip(
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> impl IntoResponse {
    let mut zip_data: Option<Vec<u8>> = None;
    let mut id = String::new();
    let mut name = String::new();
    let mut description = String::new();
    let mut tags_str = String::new();

    while let Ok(Some(field)) = multipart.next_field().await {
        let field_name = field.name().unwrap_or("").to_string();
        match field_name.as_str() {
            "file" => {
                match field.bytes().await {
                    Ok(bytes) => zip_data = Some(bytes.to_vec()),
                    Err(e) => return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": format!("Failed to read file: {e}")}))).into_response(),
                }
            }
            "id" => { id = field.text().await.unwrap_or_default(); }
            "name" => { name = field.text().await.unwrap_or_default(); }
            "description" => { description = field.text().await.unwrap_or_default(); }
            "tags" => { tags_str = field.text().await.unwrap_or_default(); }
            _ => {}
        }
    }

    if id.is_empty() || name.is_empty() {
        return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": "id and name are required"}))).into_response();
    }

    let Some(data) = zip_data else {
        return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": "No file field in upload"}))).into_response();
    };

    let limits = ExtractLimits {
        max_total_size: 50 * 1024 * 1024,
        ..Default::default()
    };

    let mut files = match upload_utils::extract_zip_to_map(&data, &limits) {
        Ok(f) => f,
        Err(e) => return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": e}))).into_response(),
    };

    // Parse TOOL.json for tool-specific fields
    let tool_json: Option<serde_json::Value> = files.remove("TOOL.json")
        .and_then(|s| serde_json::from_str(&s).ok());

    // Parse manifest.json for metadata
    let manifest: Option<serde_json::Value> = files.remove("manifest.json")
        .and_then(|s| serde_json::from_str(&s).ok());

    // Extract install_commands and check_command from TOOL.json
    let install_commands = tool_json.as_ref()
        .and_then(|t| t.get("install_commands"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let check_command = tool_json.as_ref()
        .and_then(|t| t.get("check_command"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let tags: Vec<String> = if tags_str.is_empty() {
        manifest.as_ref()
            .and_then(|m| m.get("tags"))
            .and_then(|t| serde_json::from_value::<Vec<String>>(t.clone()).ok())
            .unwrap_or_default()
    } else {
        tags_str.split(',').map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).collect()
    };

    let mut tool = claw_redis::new_tool(&id, &name, &install_commands, &check_command);
    tool.description = description;
    tool.tags = tags;

    // Apply TOOL.json fields
    if let Some(ref tj) = tool_json {
        if let Some(env_vars) = tj.get("env_vars") {
            if let Ok(vars) = serde_json::from_value::<Vec<claw_models::ToolEnvVar>>(env_vars.clone()) {
                tool.env_vars = vars;
            }
        }
        if let Some(auth) = tj.get("auth_script").and_then(|v| v.as_str()) {
            tool.auth_script = Some(auth.to_string());
        }
    }

    // Apply manifest metadata
    if let Some(ref m) = manifest {
        if let Some(v) = m.get("version").and_then(|v| v.as_str()) {
            tool.version = v.to_string();
        }
        if let Some(v) = m.get("author").and_then(|v| v.as_str()) {
            tool.author = v.to_string();
        }
        if let Some(v) = m.get("license").and_then(|v| v.as_str()) {
            tool.license = Some(v.to_string());
        }
        // If description was empty in form, use manifest
        if tool.description.is_empty() {
            if let Some(v) = m.get("description").and_then(|v| v.as_str()) {
                tool.description = v.to_string();
            }
        }
    }

    let result = match claw_redis::get_tool(&state.pool, &id).await {
        Ok(Some(existing)) => {
            let mut updated = tool;
            updated.created_at = existing.created_at;
            match claw_redis::update_tool(&state.pool, &updated).await {
                Ok(()) => Ok(updated),
                Err(e) => Err(e),
            }
        }
        _ => {
            match claw_redis::create_tool(&state.pool, &tool).await {
                Ok(()) => Ok(tool),
                Err(e) => Err(e),
            }
        }
    };

    match result {
        Ok(t) => (StatusCode::CREATED, Json(t)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    }
}
