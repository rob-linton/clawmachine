use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::get,
    Json, Router,
};

use crate::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/auth/oauth-status", get(oauth_status))
}

/// GET /auth/oauth-status — returns current OAuth token status from Redis.
async fn oauth_status(State(state): State<AppState>) -> impl IntoResponse {
    use redis::AsyncCommands;
    let mut conn = match state.pool.get().await {
        Ok(c) => c,
        Err(e) => {
            tracing::error!(error = %e, "Redis connection failed for oauth-status");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "Redis connection failed"})),
            )
                .into_response();
        }
    };

    let value: Option<String> = conn
        .get("claw:worker:oauth_status")
        .await
        .unwrap_or(None);

    match value {
        Some(json_str) => match serde_json::from_str::<serde_json::Value>(&json_str) {
            Ok(val) => (StatusCode::OK, Json(val)).into_response(),
            Err(_) => (StatusCode::OK, Json(serde_json::json!({"status": "unknown"}))).into_response(),
        },
        None => (StatusCode::OK, Json(serde_json::json!({"status": "unknown"}))).into_response(),
    }
}
