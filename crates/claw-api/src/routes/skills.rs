use axum::{
    extract::{DefaultBodyLimit, Multipart, Path, State},
    http::{header, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use std::collections::HashMap;
use std::io::Write;

use crate::AppState;
use crate::upload_utils::{self, ExtractLimits};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/skills", post(create_skill).get(list_skills))
        .route("/skills/upload", post(upload_skill_zip).layer(DefaultBodyLimit::max(104_857_600)))
        .route("/skills/{id}", get(get_skill).put(update_skill).delete(delete_skill))
        .route("/skills/{id}/download", get(download_skill))
}

#[derive(Deserialize)]
struct CreateSkillRequest {
    id: String,
    name: String,
    content: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    files: HashMap<String, String>,
}

async fn create_skill(
    State(state): State<AppState>,
    Json(req): Json<CreateSkillRequest>,
) -> impl IntoResponse {
    let skill = claw_redis::new_skill(
        &req.id, &req.name, &req.content, &req.description, req.tags, req.files,
    );
    match claw_redis::create_skill(&state.pool, &skill).await {
        Ok(()) => (StatusCode::CREATED, Json(skill)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    }
}

async fn list_skills(State(state): State<AppState>) -> impl IntoResponse {
    match claw_redis::list_skills(&state.pool).await {
        Ok(skills) => Json(serde_json::json!({"items": skills, "total": skills.len()})).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    }
}

async fn get_skill(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match claw_redis::get_skill(&state.pool, &id).await {
        Ok(Some(skill)) => Json(skill).into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    }
}

async fn update_skill(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<CreateSkillRequest>,
) -> impl IntoResponse {
    let mut skill = claw_redis::new_skill(
        &id, &req.name, &req.content, &req.description, req.tags, req.files,
    );
    if let Ok(Some(existing)) = claw_redis::get_skill(&state.pool, &id).await {
        skill.created_at = existing.created_at;
    }
    match claw_redis::update_skill(&state.pool, &skill).await {
        Ok(()) => Json(skill).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    }
}

async fn delete_skill(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match claw_redis::delete_skill(&state.pool, &id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    }
}

// --- ZIP Download ---

async fn download_skill(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let skill = match claw_redis::get_skill(&state.pool, &id).await {
        Ok(Some(s)) => s,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    };

    let buf = std::io::Cursor::new(Vec::new());
    let mut zip = zip::ZipWriter::new(buf);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    // Write SKILL.md
    if let Err(e) = zip.start_file("SKILL.md", options) {
        return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": format!("zip error: {e}")}))).into_response();
    }
    if let Err(e) = zip.write_all(skill.content.as_bytes()) {
        return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": format!("zip write error: {e}")}))).into_response();
    }

    // Write bundled files
    for (path, content) in &skill.files {
        if let Err(e) = zip.start_file(path, options) {
            tracing::warn!(path, error = %e, "Failed to add file to skill zip");
            continue;
        }
        let _ = zip.write_all(content.as_bytes());
    }

    // Write manifest.json
    let manifest = serde_json::json!({
        "format": "claw-skill-v1",
        "id": skill.id,
        "name": skill.name,
        "description": skill.description,
        "tags": skill.tags,
        "version": skill.version,
        "author": skill.author,
        "license": skill.license,
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
    let filename = format!("{}.zip", skill.id);

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

async fn upload_skill_zip(
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

    let content = files.remove("SKILL.md").unwrap_or_default();

    // Parse manifest.json for metadata if present
    let manifest: Option<serde_json::Value> = files.remove("manifest.json")
        .and_then(|s| serde_json::from_str(&s).ok());

    let tags: Vec<String> = if tags_str.is_empty() {
        // Fall back to manifest tags
        manifest.as_ref()
            .and_then(|m| m.get("tags"))
            .and_then(|t| serde_json::from_value::<Vec<String>>(t.clone()).ok())
            .unwrap_or_default()
    } else {
        tags_str.split(',').map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).collect()
    };

    let mut skill = claw_redis::new_skill(&id, &name, &content, &description, tags, files);

    // Apply manifest metadata (form fields already override via id/name/description/tags above)
    if let Some(ref m) = manifest {
        if let Some(v) = m.get("version").and_then(|v| v.as_str()) {
            skill.version = v.to_string();
        }
        if let Some(v) = m.get("author").and_then(|v| v.as_str()) {
            skill.author = v.to_string();
        }
        if let Some(v) = m.get("license").and_then(|v| v.as_str()) {
            skill.license = Some(v.to_string());
        }
        // If description was empty in form, use manifest
        if skill.description.is_empty() {
            if let Some(v) = m.get("description").and_then(|v| v.as_str()) {
                skill.description = v.to_string();
            }
        }
    }

    let result = match claw_redis::get_skill(&state.pool, &id).await {
        Ok(Some(existing)) => {
            let mut updated = skill;
            updated.created_at = existing.created_at;
            match claw_redis::update_skill(&state.pool, &updated).await {
                Ok(()) => Ok(updated),
                Err(e) => Err(e),
            }
        }
        _ => {
            match claw_redis::create_skill(&state.pool, &skill).await {
                Ok(()) => Ok(skill),
                Err(e) => Err(e),
            }
        }
    };

    match result {
        Ok(s) => (StatusCode::CREATED, Json(s)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    }
}
