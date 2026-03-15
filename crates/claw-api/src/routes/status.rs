use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::get,
    Json, Router,
};

use crate::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/status", get(health))
}

async fn health(State(state): State<AppState>) -> impl IntoResponse {
    match claw_redis::get_queue_status(&state.pool).await {
        Ok(status) => Json(serde_json::json!({
            "status": "healthy",
            "queue": status,
        }))
        .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"status": "unhealthy", "error": e.to_string()})),
        )
            .into_response(),
    }
}
