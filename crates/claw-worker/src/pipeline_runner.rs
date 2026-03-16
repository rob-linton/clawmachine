use claw_models::*;
use deadpool_redis::{redis, Pool};

/// Check if a completed job is part of a pipeline, and if so, advance to the next step.
pub async fn check_and_advance(pool: &Pool, job: &Job, result_text: &str) {
    let Some(run_id) = job.pipeline_run_id else { return };
    let Some(step_idx) = job.pipeline_step else { return };

    let Some(mut run) = claw_redis::get_pipeline_run(pool, run_id).await.ok().flatten() else {
        tracing::warn!(run_id = %run_id, "Pipeline run not found for advancing");
        return;
    };

    let Some(pipeline) = claw_redis::get_pipeline(pool, run.pipeline_id).await.ok().flatten() else {
        tracing::warn!(pipeline_id = %run.pipeline_id, "Pipeline not found");
        return;
    };

    let next_step = step_idx + 1;

    if next_step >= pipeline.steps.len() {
        // Pipeline complete
        run.status = PipelineStatus::Completed;
        run.completed_at = Some(chrono::Utc::now());
        run.current_step = step_idx;
        claw_redis::update_pipeline_run(pool, &run).await.ok();
        tracing::info!(run_id = %run_id, "Pipeline completed");

        // Release pipeline workspace lock if held
        if let Some(ws_id) = run.workspace_id {
            claw_redis::release_workspace_lock(pool, ws_id, run_id).await.ok();
        }
        return;
    }

    // Submit next step
    let step = &pipeline.steps[next_step];
    let prompt = step.prompt.replace("{{previous_result}}", result_text);

    let req = CreateJobRequest {
        prompt,
        skill_ids: step.skill_ids.clone(),
        skill_tags: vec![],
        working_dir: None,
        model: step.model.clone(),
        max_budget_usd: None,
        allowed_tools: None,
        output_dest: OutputDest::Redis,
        tags: vec![format!("pipeline:{}", pipeline.name), format!("step:{}", next_step)],
        priority: Some(5),
        timeout_secs: step.timeout_secs,
        workspace_id: pipeline.workspace_id,
    };

    match claw_redis::submit_job(pool, &req, JobSource::Api).await {
        Ok(mut next_job) => {
            // Tag with pipeline info
            next_job.pipeline_run_id = Some(run_id);
            next_job.pipeline_step = Some(next_step);
            let job_json = serde_json::to_string(&next_job).unwrap_or_default();
            if let Ok(mut conn) = pool.get().await {
                let _: Result<(), _> = redis::AsyncCommands::set(
                    &mut *conn,
                    format!("claw:job:{}", next_job.id),
                    &job_json,
                ).await;
            }

            // Update run
            run.current_step = next_step;
            if next_step < run.step_jobs.len() {
                run.step_jobs[next_step] = Some(next_job.id);
            }
            claw_redis::update_pipeline_run(pool, &run).await.ok();

            tracing::info!(
                run_id = %run_id,
                step = next_step,
                job_id = %next_job.id,
                "Pipeline step submitted"
            );
        }
        Err(e) => {
            run.status = PipelineStatus::Failed;
            run.error = Some(format!("Failed to submit step {}: {}", next_step, e));
            run.completed_at = Some(chrono::Utc::now());
            claw_redis::update_pipeline_run(pool, &run).await.ok();
            tracing::error!(run_id = %run_id, step = next_step, error = %e, "Pipeline step submission failed");

            if let Some(ws_id) = run.workspace_id {
                claw_redis::release_workspace_lock(pool, ws_id, run_id).await.ok();
            }
        }
    }
}

/// Mark a pipeline as failed when a step job fails.
pub async fn mark_failed(pool: &Pool, job: &Job, error: &str) {
    let Some(run_id) = job.pipeline_run_id else { return };

    if let Ok(Some(mut run)) = claw_redis::get_pipeline_run(pool, run_id).await {
        run.status = PipelineStatus::Failed;
        run.error = Some(error.to_string());
        run.completed_at = Some(chrono::Utc::now());
        claw_redis::update_pipeline_run(pool, &run).await.ok();

        if let Some(ws_id) = run.workspace_id {
            claw_redis::release_workspace_lock(pool, ws_id, run_id).await.ok();
        }

        tracing::info!(run_id = %run_id, "Pipeline marked failed");
    }
}
