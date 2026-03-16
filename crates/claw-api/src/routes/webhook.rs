use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::post,
    Json, Router,
};
use claw_models::*;

use crate::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/webhook/submit", post(webhook_submit))
}

/// Generic inbound webhook — same body as POST /jobs but via a webhook-friendly URL.
async fn webhook_submit(
    State(state): State<AppState>,
    Json(req): Json<CreateJobRequest>,
) -> impl IntoResponse {
    match claw_redis::submit_job(&state.pool, &req, JobSource::Api).await {
        Ok(job) => {
            tracing::info!(job_id = %job.id, "Job submitted via webhook");
            (StatusCode::ACCEPTED, Json(serde_json::json!({
                "job_id": job.id,
                "status": "pending",
            }))).into_response()
        }
        Err(e) => {
            (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response()
        }
    }
}
