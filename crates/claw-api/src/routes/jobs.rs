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
    Json(mut req): Json<CreateJobRequest>,
) -> impl IntoResponse {
    // If template_id is set, load template and merge fields (request fields override template)
    if let Some(tmpl_id) = req.template_id {
        if let Ok(Some(tmpl)) = claw_redis::get_job_template(&state.pool, tmpl_id).await {
            if req.prompt.is_empty() { req.prompt = tmpl.prompt; }
            if req.skill_ids.is_empty() { req.skill_ids = tmpl.skill_ids; }
            if req.model.is_none() { req.model = tmpl.model; }
            if req.workspace_id.is_none() { req.workspace_id = tmpl.workspace_id; }
            if req.timeout_secs.is_none() { req.timeout_secs = tmpl.timeout_secs; }
            if req.allowed_tools.is_none() { req.allowed_tools = tmpl.allowed_tools; }
            if req.priority.is_none() { req.priority = Some(tmpl.priority); }
            if req.tags.is_empty() { req.tags = tmpl.tags; }
        }
    }
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
    offset: Option<usize>,
    workspace_id: Option<Uuid>,
}

async fn list_jobs(
    State(state): State<AppState>,
    Query(q): Query<ListQuery>,
) -> impl IntoResponse {
    let status_filter = q.status.and_then(|s| s.parse::<JobStatus>().ok());
    // Fetch more than we need so offset works correctly
    let fetch_limit = q.offset.unwrap_or(0) + q.limit.unwrap_or(20).min(100);

    match claw_redis::list_jobs(&state.pool, status_filter, fetch_limit, q.workspace_id).await {
        Ok(jobs) => {
            let total = jobs.len();
            let offset = q.offset.unwrap_or(0);
            let limit = q.limit.unwrap_or(20).min(100);
            let page: Vec<_> = jobs.into_iter().skip(offset).take(limit).collect();
            Json(serde_json::json!({
                "items": page,
                "total": total,
                "offset": offset,
                "limit": limit,
            })).into_response()
        }
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
