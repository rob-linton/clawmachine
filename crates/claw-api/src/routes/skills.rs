use axum::{
    extract::{DefaultBodyLimit, Multipart, Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use std::collections::HashMap;

use crate::AppState;
use crate::upload_utils::{self, ExtractLimits};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/skills", post(create_skill).get(list_skills))
        .route("/skills/upload", post(upload_skill_zip).layer(DefaultBodyLimit::max(104_857_600)))
        .route("/skills/{id}", get(get_skill).put(update_skill).delete(delete_skill))
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

    let tags: Vec<String> = if tags_str.is_empty() {
        vec![]
    } else {
        tags_str.split(',').map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).collect()
    };

    let skill = claw_redis::new_skill(&id, &name, &content, &description, tags, files);

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
