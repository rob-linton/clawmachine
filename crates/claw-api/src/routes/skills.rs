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
        .route("/skills/install-from-url", post(install_skill_from_url))
        .route("/skills/{id}", get(get_skill).put(update_skill).delete(delete_skill))
        .route("/skills/{id}/download", get(download_skill))
        .route("/skills/{id}/update-from-source", post(update_skill_from_source))
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

// --- Install from URL ---

#[derive(Deserialize)]
struct InstallFromUrlRequest {
    url: String,
    #[serde(default)]
    path: Option<String>,
}

/// Fetch skill files from a URL (git repo or ZIP), parse manifest + SKILL.md, upsert to Redis.
async fn fetch_skill_from_url(url: &str, subpath: Option<&str>) -> Result<(HashMap<String, String>, Option<serde_json::Value>), String> {
    if !url.starts_with("https://") {
        return Err("URL must start with https:// (SSRF prevention)".into());
    }

    let is_git = url.contains(".git")
        || url.contains("github.com")
        || url.contains("gitlab.com");
    let is_zip = url.ends_with(".zip");

    if is_zip {
        // Download ZIP
        let resp = reqwest::get(url).await.map_err(|e| format!("Download failed: {e}"))?;
        if !resp.status().is_success() {
            return Err(format!("Download failed: HTTP {}", resp.status()));
        }
        // Check content-length (100MB limit)
        if let Some(cl) = resp.content_length() {
            if cl > 100 * 1024 * 1024 {
                return Err("ZIP file too large (max 100MB)".into());
            }
        }
        let bytes = resp.bytes().await.map_err(|e| format!("Download read failed: {e}"))?;
        let limits = ExtractLimits {
            max_total_size: 50 * 1024 * 1024,
            ..Default::default()
        };
        let files = upload_utils::extract_zip_to_map(&bytes, &limits)?;
        let manifest: Option<serde_json::Value> = files.get("manifest.json")
            .and_then(|s| serde_json::from_str(s).ok());
        Ok((files, manifest))
    } else if is_git {
        // Git clone to temp dir
        let temp_dir = tempfile::tempdir().map_err(|e| format!("Failed to create temp dir: {e}"))?;
        let temp_path = temp_dir.path();

        let output = tokio::process::Command::new("git")
            .args(["clone", "--depth", "1", "--single-branch", url, &temp_path.to_string_lossy()])
            .output()
            .await
            .map_err(|e| format!("git clone failed: {e}"))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            // If it's a GitHub URL without .git, try downloading as ZIP
            if url.contains("github.com") && !url.contains(".git") {
                let zip_url = format!("{}/archive/refs/heads/main.zip", url.trim_end_matches('/'));
                return Box::pin(fetch_skill_from_url(&zip_url, subpath)).await;
            }
            return Err(format!("git clone failed: {stderr}"));
        }

        // Look in subpath if specified
        let base_dir = if let Some(sp) = subpath {
            temp_path.join(sp)
        } else {
            temp_path.to_path_buf()
        };

        if !base_dir.exists() {
            return Err(format!("Path '{}' not found in repository", subpath.unwrap_or("")));
        }

        // Read files from disk into HashMap
        let mut files = HashMap::new();
        read_dir_recursive(&base_dir, &base_dir, &mut files).await?;

        let manifest: Option<serde_json::Value> = files.get("manifest.json")
            .and_then(|s| serde_json::from_str(s).ok());

        Ok((files, manifest))
    } else {
        Err("URL must be a git repository (github.com/gitlab.com) or a .zip file".into())
    }
}

/// Recursively read all text files from a directory into a HashMap.
async fn read_dir_recursive(
    base: &std::path::Path,
    dir: &std::path::Path,
    files: &mut HashMap<String, String>,
) -> Result<(), String> {
    let mut entries = tokio::fs::read_dir(dir).await.map_err(|e| format!("Read dir failed: {e}"))?;
    while let Some(entry) = entries.next_entry().await.map_err(|e| format!("Read entry failed: {e}"))? {
        let path = entry.path();
        let file_type = entry.file_type().await.map_err(|e| format!("File type failed: {e}"))?;
        if file_type.is_dir() {
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            if name == ".git" || name == "node_modules" || name == ".claw" {
                continue;
            }
            Box::pin(read_dir_recursive(base, &path, files)).await?;
        } else if file_type.is_file() {
            let rel = path.strip_prefix(base).map_err(|e| format!("Strip prefix failed: {e}"))?;
            let rel_str = rel.to_string_lossy().to_string();
            // Skip hidden files and large files
            if rel_str.starts_with('.') || rel_str.contains("/.") {
                continue;
            }
            let metadata = tokio::fs::metadata(&path).await.map_err(|e| format!("Metadata failed: {e}"))?;
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

/// Build a skill from fetched files + manifest, with source_url set.
fn build_skill_from_fetched(
    mut files: HashMap<String, String>,
    manifest: Option<serde_json::Value>,
    source_url: &str,
) -> Result<claw_models::Skill, String> {
    let content = files.remove("SKILL.md").unwrap_or_default();

    // Remove manifest.json from bundled files
    files.remove("manifest.json");

    let id = manifest.as_ref()
        .and_then(|m| m.get("id")).and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| "manifest.json must contain an 'id' field".to_string())?;

    let name = manifest.as_ref()
        .and_then(|m| m.get("name")).and_then(|v| v.as_str())
        .unwrap_or(&id).to_string();

    let description = manifest.as_ref()
        .and_then(|m| m.get("description")).and_then(|v| v.as_str())
        .unwrap_or("").to_string();

    let tags: Vec<String> = manifest.as_ref()
        .and_then(|m| m.get("tags"))
        .and_then(|t| serde_json::from_value::<Vec<String>>(t.clone()).ok())
        .unwrap_or_default();

    let mut skill = claw_redis::new_skill(&id, &name, &content, &description, tags, files);

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
    }

    skill.source_url = Some(source_url.to_string());

    Ok(skill)
}

async fn install_skill_from_url(
    State(state): State<AppState>,
    Json(req): Json<InstallFromUrlRequest>,
) -> impl IntoResponse {
    let (files, manifest) = match fetch_skill_from_url(&req.url, req.path.as_deref()).await {
        Ok(r) => r,
        Err(e) => return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": e}))).into_response(),
    };

    let skill = match build_skill_from_fetched(files, manifest, &req.url) {
        Ok(s) => s,
        Err(e) => return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": e}))).into_response(),
    };

    // Upsert
    let result = match claw_redis::get_skill(&state.pool, &skill.id).await {
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

async fn update_skill_from_source(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let existing = match claw_redis::get_skill(&state.pool, &id).await {
        Ok(Some(s)) => s,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    };

    let source_url = match &existing.source_url {
        Some(url) if !url.is_empty() => url.clone(),
        _ => return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": "No source URL set for this skill"}))).into_response(),
    };

    let (files, manifest) = match fetch_skill_from_url(&source_url, None).await {
        Ok(r) => r,
        Err(e) => return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": e}))).into_response(),
    };

    let mut skill = match build_skill_from_fetched(files, manifest, &source_url) {
        Ok(s) => s,
        Err(e) => return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": e}))).into_response(),
    };

    // Preserve created_at and ID
    skill.id = existing.id;
    skill.created_at = existing.created_at;

    match claw_redis::update_skill(&state.pool, &skill).await {
        Ok(()) => Json(skill).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    }
}
