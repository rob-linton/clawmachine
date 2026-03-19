use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};

use crate::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/tools", post(create_tool).get(list_tools))
        .route("/tools/{id}", get(get_tool).put(update_tool).delete(delete_tool))
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
