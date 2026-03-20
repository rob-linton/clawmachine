use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;

use crate::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/auth/oauth-status", get(oauth_status))
        .route("/auth/oauth-login", post(oauth_login))
        .route("/auth/oauth-code", post(oauth_code))
}

#[derive(Deserialize)]
struct OAuthCodeRequest {
    code: String,
}

/// POST /auth/oauth-code — submit the authentication code from the browser to claude auth login.
async fn oauth_code(
    State(state): State<AppState>,
    Json(req): Json<OAuthCodeRequest>,
) -> impl IntoResponse {
    use redis::AsyncCommands;
    let mut conn = match state.pool.get().await {
        Ok(c) => c,
        Err(e) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": format!("{e}")}))).into_response();
        }
    };
    if let Err(e) = conn.publish::<_, _, ()>("claw:oauth-login:code", &req.code).await {
        return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": format!("{e}")}))).into_response();
    }
    tracing::info!("OAuth authentication code submitted");
    (StatusCode::OK, Json(serde_json::json!({"status": "submitted"}))).into_response()
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

#[derive(Deserialize)]
struct OAuthLoginRequest {
    email: String,
}

/// POST /auth/oauth-login — publish login request to worker, return request_id.
/// The worker runs `claude auth login`, provides the OAuth URL via status endpoint,
/// and the user completes login in their own browser.
async fn oauth_login(
    State(state): State<AppState>,
    Json(req): Json<OAuthLoginRequest>,
) -> impl IntoResponse {
    use redis::AsyncCommands;

    let request_id = uuid::Uuid::new_v4().to_string();

    let payload = serde_json::json!({
        "email": req.email,
        "request_id": request_id,
    });

    let mut conn = match state.pool.get().await {
        Ok(c) => c,
        Err(e) => {
            tracing::error!(error = %e, "Redis connection failed for oauth-login");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "Redis connection failed"})),
            )
                .into_response();
        }
    };

    if let Err(e) = conn
        .publish::<_, _, ()>("claw:oauth-login:request", payload.to_string())
        .await
    {
        tracing::error!(error = %e, "Failed to publish oauth login request");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "Failed to publish request"})),
        )
            .into_response();
    }

    tracing::info!(request_id, "OAuth login request published");

    (
        StatusCode::ACCEPTED,
        Json(serde_json::json!({
            "request_id": request_id,
            "message": "OAuth login initiated. Check the Settings page for the login URL.",
        })),
    )
        .into_response()
}
