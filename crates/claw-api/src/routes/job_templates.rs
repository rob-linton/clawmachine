use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use claw_models::*;
use uuid::Uuid;

use crate::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/job-templates", post(create_template).get(list_templates))
        .route("/job-templates/{id}", get(get_template).put(update_template).delete(delete_template))
        .route("/job-templates/{id}/run", post(run_template))
}

async fn create_template(
    State(state): State<AppState>,
    Json(req): Json<CreateJobTemplateRequest>,
) -> impl IntoResponse {
    match claw_redis::create_job_template(&state.pool, &req).await {
        Ok(t) => (StatusCode::CREATED, Json(t)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    }
}

#[derive(serde::Deserialize)]
struct ListQuery {
    limit: Option<usize>,
    offset: Option<usize>,
}

async fn list_templates(
    State(state): State<AppState>,
    axum::extract::Query(q): axum::extract::Query<ListQuery>,
) -> impl IntoResponse {
    match claw_redis::list_job_templates(&state.pool).await {
        Ok(ts) => {
            let total = ts.len();
            let offset = q.offset.unwrap_or(0);
            let limit = q.limit.unwrap_or(50).min(100);
            let page: Vec<_> = ts.into_iter().skip(offset).take(limit).collect();
            Json(serde_json::json!({"items": page, "total": total, "offset": offset, "limit": limit})).into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    }
}

async fn get_template(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    match claw_redis::get_job_template(&state.pool, id).await {
        Ok(Some(t)) => Json(t).into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    }
}

async fn update_template(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(req): Json<CreateJobTemplateRequest>,
) -> impl IntoResponse {
    let existing = match claw_redis::get_job_template(&state.pool, id).await {
        Ok(Some(t)) => t,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    };

    let updated = JobTemplate {
        id,
        name: req.name,
        description: req.description,
        prompt: req.prompt,
        skill_ids: req.skill_ids,
        workspace_id: req.workspace_id,
        model: req.model,
        timeout_secs: req.timeout_secs,
        allowed_tools: req.allowed_tools,
        output_dest: req.output_dest,
        tags: req.tags,
        priority: req.priority.unwrap_or(5),
        created_at: existing.created_at,
        updated_at: chrono::Utc::now(),
    };

    match claw_redis::update_job_template(&state.pool, &updated).await {
        Ok(()) => Json(updated).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    }
}

async fn delete_template(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    match claw_redis::delete_job_template(&state.pool, id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => {
            let msg = e.to_string();
            if msg.contains("referenced") {
                (StatusCode::CONFLICT, Json(serde_json::json!({"error": msg}))).into_response()
            } else {
                (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": msg}))).into_response()
            }
        }
    }
}

/// Run a template immediately as a job.
async fn run_template(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    let template = match claw_redis::get_job_template(&state.pool, id).await {
        Ok(Some(t)) => t,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    };

    let req = CreateJobRequest {
        prompt: template.prompt,
        skill_ids: template.skill_ids,
        skill_tags: vec![],
        working_dir: None,
        model: template.model,
        max_budget_usd: None,
        allowed_tools: template.allowed_tools,
        output_dest: template.output_dest,
        tags: template.tags,
        priority: Some(template.priority),
        timeout_secs: template.timeout_secs,
        workspace_id: template.workspace_id,
        template_id: Some(id),
    };

    match claw_redis::submit_job(&state.pool, &req, JobSource::Api).await {
        Ok(job) => (StatusCode::CREATED, Json(serde_json::json!({
            "job_id": job.id,
            "template_id": id,
            "status": "pending",
        }))).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    }
}
