use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use claw_models::*;
use serde::Deserialize;

use crate::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/skills", post(create_skill).get(list_skills))
        .route("/skills/{id}", get(get_skill).put(update_skill).delete(delete_skill))
}

#[derive(Deserialize)]
struct CreateSkillRequest {
    id: String,
    name: String,
    skill_type: SkillType,
    content: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    tags: Vec<String>,
}

async fn create_skill(
    State(state): State<AppState>,
    Json(req): Json<CreateSkillRequest>,
) -> impl IntoResponse {
    let skill = claw_redis::new_skill(
        &req.id, &req.name, req.skill_type, &req.content, &req.description, req.tags,
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
        &id, &req.name, req.skill_type, &req.content, &req.description, req.tags,
    );
    // Preserve original created_at if updating
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
