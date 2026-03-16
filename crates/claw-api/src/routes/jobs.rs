use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use claw_models::*;
use serde::Deserialize;
use uuid::Uuid;

use crate::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/jobs", post(create_job).get(list_jobs))
        .route("/jobs/{id}", get(get_job).delete(delete_job))
        .route("/jobs/{id}/result", get(get_result))
        .route("/jobs/{id}/logs", get(get_logs))
        .route("/jobs/{id}/cancel", post(cancel_job))
}

async fn create_job(
    State(state): State<AppState>,
    Json(req): Json<CreateJobRequest>,
) -> impl IntoResponse {
    match claw_redis::submit_job(&state.pool, &req, JobSource::Api).await {
        Ok(job) => {
            let resp = CreateJobResponse {
                id: job.id,
                status: job.status,
                created_at: job.created_at,
            };
            (StatusCode::CREATED, Json(resp)).into_response()
        }
        Err(e) => {
            tracing::error!("Failed to submit job: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response()
        }
    }
}

#[derive(Deserialize)]
struct ListQuery {
    status: Option<String>,
    limit: Option<usize>,
    workspace_id: Option<Uuid>,
}

async fn list_jobs(
    State(state): State<AppState>,
    Query(q): Query<ListQuery>,
) -> impl IntoResponse {
    let status_filter = q.status.and_then(|s| s.parse::<JobStatus>().ok());
    let limit = q.limit.unwrap_or(20).min(100);

    match claw_redis::list_jobs(&state.pool, status_filter, limit, q.workspace_id).await {
        Ok(jobs) => Json(serde_json::json!({
            "items": jobs,
            "total": jobs.len(),
        })).into_response(),
        Err(e) => {
            (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response()
        }
    }
}

async fn get_job(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    match claw_redis::get_job(&state.pool, id).await {
        Ok(job) => Json(job).into_response(),
        Err(claw_redis::RedisError::NotFound(_)) => StatusCode::NOT_FOUND.into_response(),
        Err(e) => {
            (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response()
        }
    }
}

async fn get_result(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    match claw_redis::get_result(&state.pool, id).await {
        Ok(result) => Json(result).into_response(),
        Err(claw_redis::RedisError::NotFound(_)) => StatusCode::NOT_FOUND.into_response(),
        Err(e) => {
            (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response()
        }
    }
}

#[derive(Deserialize)]
struct LogsQuery {
    offset: Option<usize>,
    limit: Option<usize>,
}

async fn get_logs(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Query(q): Query<LogsQuery>,
) -> impl IntoResponse {
    let offset = q.offset.unwrap_or(0);
    let limit = q.limit.unwrap_or(500);

    match claw_redis::get_logs(&state.pool, id, offset, limit).await {
        Ok(lines) => Json(serde_json::json!({
            "job_id": id,
            "lines": lines,
            "total": lines.len(),
            "offset": offset,
        })).into_response(),
        Err(e) => {
            (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response()
        }
    }
}

async fn cancel_job(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    // Check job exists and is cancellable
    match claw_redis::get_job(&state.pool, id).await {
        Ok(job) => {
            match job.status {
                JobStatus::Completed | JobStatus::Failed | JobStatus::Cancelled => {
                    return (StatusCode::CONFLICT, Json(serde_json::json!({
                        "error": format!("Job is already {}", job.status)
                    }))).into_response();
                }
                JobStatus::Running => {
                    // Set cancel flag — worker will pick it up
                    claw_redis::set_cancel_flag(&state.pool, id).await.ok();
                }
                JobStatus::Pending => {
                    // Cancel immediately
                    if let Err(e) = claw_redis::cancel_job(&state.pool, id).await {
                        return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response();
                    }
                }
            }
            Json(serde_json::json!({"id": id, "status": "cancelled"})).into_response()
        }
        Err(claw_redis::RedisError::NotFound(_)) => StatusCode::NOT_FOUND.into_response(),
        Err(e) => {
            (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response()
        }
    }
}

async fn delete_job(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    match claw_redis::get_job(&state.pool, id).await {
        Ok(job) => {
            match job.status {
                JobStatus::Pending | JobStatus::Running => {
                    return (StatusCode::CONFLICT, Json(serde_json::json!({
                        "error": "Cannot delete a pending or running job. Cancel it first."
                    }))).into_response();
                }
                _ => {}
            }
            if let Err(e) = claw_redis::delete_job(&state.pool, id).await {
                return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response();
            }
            StatusCode::NO_CONTENT.into_response()
        }
        Err(claw_redis::RedisError::NotFound(_)) => StatusCode::NOT_FOUND.into_response(),
        Err(e) => {
            (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response()
        }
    }
}
