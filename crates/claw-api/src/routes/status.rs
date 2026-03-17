use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use tokio::process::Command;

use crate::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/status", get(health))
}

async fn health(State(state): State<AppState>) -> impl IntoResponse {
    let queue_status = claw_redis::get_queue_status(&state.pool).await;

    // Check Docker availability
    let docker_available = Command::new("docker")
        .args(["version", "--format", "{{.Server.Version}}"])
        .output()
        .await
        .map(|o| o.status.success())
        .unwrap_or(false);

    // Check if sandbox image exists
    let sandbox_image = claw_redis::get_config(&state.pool, "sandbox_image")
        .await
        .unwrap_or_else(|_| "claw-sandbox:latest".to_string());
    let sandbox_image_ready = if docker_available {
        Command::new("docker")
            .args(["image", "inspect", &sandbox_image])
            .output()
            .await
            .map(|o| o.status.success())
            .unwrap_or(false)
    } else {
        false
    };

    // Get execution backend config
    let execution_backend = claw_redis::get_config(&state.pool, "execution_backend")
        .await
        .unwrap_or_else(|_| "local".to_string());

    // Count active workers via heartbeat keys
    let worker_count = claw_redis::count_active_workers(&state.pool).await.unwrap_or(0);

    match queue_status {
        Ok(status) => Json(serde_json::json!({
            "status": "healthy",
            "queue": status,
            "docker_available": docker_available,
            "sandbox_image_ready": sandbox_image_ready,
            "execution_backend": execution_backend,
            "worker_count": worker_count,
        }))
        .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"status": "unhealthy", "error": e.to_string()})),
        )
            .into_response(),
    }
}
