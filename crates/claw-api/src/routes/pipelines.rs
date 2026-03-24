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
        .route("/pipelines", post(create_pipeline).get(list_pipelines))
        .route("/pipelines/{id}", get(get_pipeline).put(update_pipeline_handler).delete(delete_pipeline))
        .route("/pipelines/{id}/run", post(run_pipeline))
        .route("/pipeline-runs", get(list_runs))
        .route("/pipeline-runs/{id}", get(get_run))
}

async fn create_pipeline(
    State(state): State<AppState>,
    Json(req): Json<CreatePipelineRequest>,
) -> impl IntoResponse {
    if req.steps.is_empty() {
        return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": "Pipeline must have at least one step"}))).into_response();
    }
    match claw_redis::create_pipeline(&state.pool, &req).await {
        Ok(p) => (StatusCode::CREATED, Json(p)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    }
}

#[derive(serde::Deserialize)]
struct ListQuery {
    limit: Option<usize>,
    offset: Option<usize>,
}

async fn list_pipelines(
    State(state): State<AppState>,
    axum::extract::Query(q): axum::extract::Query<ListQuery>,
) -> impl IntoResponse {
    match claw_redis::list_pipelines(&state.pool).await {
        Ok(ps) => {
            let total = ps.len();
            let offset = q.offset.unwrap_or(0);
            let limit = q.limit.unwrap_or(50).min(100);
            let page: Vec<_> = ps.into_iter().skip(offset).take(limit).collect();
            Json(serde_json::json!({"items": page, "total": total, "offset": offset, "limit": limit})).into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    }
}

async fn get_pipeline(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    match claw_redis::get_pipeline(&state.pool, id).await {
        Ok(Some(p)) => Json(p).into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    }
}

async fn update_pipeline_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(req): Json<CreatePipelineRequest>,
) -> impl IntoResponse {
    let existing = match claw_redis::get_pipeline(&state.pool, id).await {
        Ok(Some(p)) => p,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    };
    if req.steps.is_empty() {
        return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": "Pipeline must have at least one step"}))).into_response();
    }
    let updated = Pipeline {
        id: existing.id,
        name: req.name,
        description: req.description,
        workspace_id: req.workspace_id,
        steps: req.steps,
        created_at: existing.created_at,
    };
    match claw_redis::update_pipeline(&state.pool, &updated).await {
        Ok(()) => Json(updated).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    }
}

async fn delete_pipeline(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    match claw_redis::delete_pipeline(&state.pool, id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    }
}

/// Trigger a pipeline run: creates a PipelineRun and submits the first step as a job.
async fn run_pipeline(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    let pipeline = match claw_redis::get_pipeline(&state.pool, id).await {
        Ok(Some(p)) => p,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    };

    if pipeline.steps.is_empty() {
        return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": "Pipeline has no steps"}))).into_response();
    }

    // Create pipeline run
    let run = match claw_redis::create_pipeline_run(&state.pool, &pipeline).await {
        Ok(r) => r,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    };

    // Acquire workspace lock for the entire pipeline run
    if let Some(ws_id) = pipeline.workspace_id {
        let ttl = pipeline.steps.iter()
            .map(|s| s.timeout_secs.unwrap_or(1800))
            .sum::<u64>() + 120; // sum of all step timeouts + buffer
        match claw_redis::acquire_workspace_lock(&state.pool, ws_id, run.id, ttl).await {
            Ok(true) => {}
            Ok(false) => {
                return (StatusCode::CONFLICT, Json(serde_json::json!({"error": "Workspace is busy"}))).into_response();
            }
            Err(e) => {
                return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": format!("Lock failed: {e}")}))).into_response();
            }
        }
    }

    // Submit first step as a job
    let step = &pipeline.steps[0];
    let req = CreateJobRequest {
        prompt: step.prompt.clone(),
        skill_ids: step.skill_ids.clone(),
        skill_tags: vec![],
        tool_ids: step.tool_ids.clone(),
        working_dir: None,
        model: step.model.clone(),
        max_budget_usd: None,
        allowed_tools: None,
        output_dest: OutputDest::Redis,
        tags: vec![format!("pipeline:{}", pipeline.name)],
        priority: Some(5),
        timeout_secs: step.timeout_secs,
        workspace_id: pipeline.workspace_id,
        template_id: step.template_id,
    };

    match claw_redis::submit_job(&state.pool, &req, JobSource::Api).await {
        Ok(mut job) => {
            // Tag job with pipeline info
            job.pipeline_run_id = Some(run.id);
            job.pipeline_step = Some(0);
            // Persist the pipeline fields
            let job_json = serde_json::to_string(&job).unwrap_or_default();
            if let Ok(mut conn) = state.pool.get().await {
                let _: Result<(), _> = deadpool_redis::redis::AsyncCommands::set(&mut *conn, format!("claw:job:{}", job.id), &job_json).await;
            }

            // Update run with first job ID
            let mut updated_run = run.clone();
            updated_run.step_jobs[0] = Some(job.id);
            claw_redis::update_pipeline_run(&state.pool, &updated_run).await.ok();

            (StatusCode::CREATED, Json(serde_json::json!({
                "run_id": run.id,
                "pipeline_id": id,
                "first_job_id": job.id,
                "status": "running",
            }))).into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    }
}

#[derive(serde::Deserialize)]
struct ListRunsQuery {
    limit: Option<usize>,
    offset: Option<usize>,
}

async fn list_runs(
    State(state): State<AppState>,
    axum::extract::Query(q): axum::extract::Query<ListRunsQuery>,
) -> impl IntoResponse {
    match claw_redis::list_pipeline_runs(&state.pool, None).await {
        Ok(runs) => {
            let total = runs.len();
            let offset = q.offset.unwrap_or(0);
            let limit = q.limit.unwrap_or(50).min(100);
            let page: Vec<_> = runs.into_iter().skip(offset).take(limit).collect();
            Json(serde_json::json!({"items": page, "total": total, "offset": offset, "limit": limit})).into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    }
}

async fn get_run(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    match claw_redis::get_pipeline_run(&state.pool, id).await {
        Ok(Some(r)) => Json(r).into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    }
}
