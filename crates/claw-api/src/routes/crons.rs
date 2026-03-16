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
        .route("/crons", post(create_cron).get(list_crons))
        .route("/crons/{id}", get(get_cron).put(update_cron).delete(delete_cron))
        .route("/crons/{id}/trigger", post(trigger_cron))
}

async fn create_cron(
    State(state): State<AppState>,
    Json(req): Json<CreateCronRequest>,
) -> impl IntoResponse {
    // Validate cron expression
    if cron::Schedule::from_str(&req.schedule).is_err() {
        return (StatusCode::UNPROCESSABLE_ENTITY, Json(serde_json::json!({
            "error": format!("Invalid cron expression: {}", req.schedule)
        }))).into_response();
    }

    match claw_redis::create_cron(&state.pool, &req).await {
        Ok(c) => (StatusCode::CREATED, Json(c)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    }
}

async fn list_crons(State(state): State<AppState>) -> impl IntoResponse {
    match claw_redis::list_crons(&state.pool).await {
        Ok(crons) => Json(serde_json::json!({"items": crons, "total": crons.len()})).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    }
}

async fn get_cron(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    match claw_redis::get_cron(&state.pool, id).await {
        Ok(Some(c)) => Json(c).into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    }
}

async fn update_cron(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(req): Json<CreateCronRequest>,
) -> impl IntoResponse {
    // Validate cron expression
    if cron::Schedule::from_str(&req.schedule).is_err() {
        return (StatusCode::UNPROCESSABLE_ENTITY, Json(serde_json::json!({
            "error": format!("Invalid cron expression: {}", req.schedule)
        }))).into_response();
    }

    let existing = match claw_redis::get_cron(&state.pool, id).await {
        Ok(Some(c)) => c,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    };

    let updated = CronSchedule {
        id,
        name: req.name,
        schedule: req.schedule,
        enabled: req.enabled,
        prompt: req.prompt,
        skill_ids: req.skill_ids,
        working_dir: req.working_dir.unwrap_or_else(|| ".".into()),
        model: req.model,
        max_budget_usd: req.max_budget_usd,
        output_dest: req.output_dest,
        tags: req.tags,
        priority: req.priority.unwrap_or(5),
        last_run: existing.last_run,
        last_job_id: existing.last_job_id,
        created_at: existing.created_at,
    };

    match claw_redis::update_cron(&state.pool, &updated).await {
        Ok(()) => Json(updated).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    }
}

async fn delete_cron(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    match claw_redis::delete_cron(&state.pool, id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    }
}

async fn trigger_cron(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    let cron = match claw_redis::get_cron(&state.pool, id).await {
        Ok(Some(c)) => c,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    };

    let req = CreateJobRequest {
        prompt: cron.prompt,
        skill_ids: cron.skill_ids,
        skill_tags: vec![],
        working_dir: Some(cron.working_dir),
        model: cron.model,
        max_budget_usd: cron.max_budget_usd,
        allowed_tools: None,
        output_dest: cron.output_dest,
        tags: cron.tags,
        priority: Some(cron.priority),
        timeout_secs: None,
    };

    match claw_redis::submit_job(&state.pool, &req, JobSource::Cron).await {
        Ok(job) => {
            claw_redis::record_cron_fire(&state.pool, id, job.id).await.ok();
            (StatusCode::CREATED, Json(serde_json::json!({
                "job_id": job.id,
                "cron_id": id,
                "status": "pending",
            }))).into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    }
}

use std::str::FromStr;
