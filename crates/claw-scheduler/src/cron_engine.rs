use chrono::Utc;
use cron::Schedule;
use deadpool_redis::Pool;
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use claw_models::*;

/// Cron engine: periodically checks all cron schedules and submits jobs when due.
pub async fn run(pool: Pool, check_interval_secs: u64, shutdown: Arc<AtomicBool>) {
    tracing::info!("Cron engine started (checking every {check_interval_secs}s)");

    let mut interval = tokio::time::interval(std::time::Duration::from_secs(check_interval_secs));

    loop {
        interval.tick().await;
        if shutdown.load(Ordering::Relaxed) {
            tracing::info!("Cron engine shutting down");
            break;
        }

        if let Err(e) = check_and_fire(&pool).await {
            tracing::error!(error = %e, "Cron check failed");
        }
    }
}

async fn check_and_fire(pool: &Pool) -> Result<(), Box<dyn std::error::Error>> {
    let crons = claw_redis::list_crons(pool).await?;
    let now = Utc::now();

    for cron in &crons {
        if !cron.enabled {
            continue;
        }

        // Parse the cron expression
        let schedule = match Schedule::from_str(&cron.schedule) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(cron_id = %cron.id, schedule = %cron.schedule, error = %e, "Invalid cron expression");
                continue;
            }
        };

        // Check if it should have fired since last_run
        let should_fire = match cron.last_run {
            Some(last) => {
                // Find the next occurrence after last_run
                if let Some(next) = schedule.after(&last).next() {
                    next <= now
                } else {
                    false
                }
            }
            None => {
                // Never run — fire if there's a past occurrence
                true
            }
        };

        if !should_fire {
            continue;
        }

        // Dedup: don't fire if last_job_id is still pending/running
        if let Some(last_job_id) = cron.last_job_id {
            if let Ok(job) = claw_redis::get_job(pool, last_job_id).await {
                if job.status == JobStatus::Pending || job.status == JobStatus::Running {
                    tracing::debug!(
                        cron_id = %cron.id,
                        last_job_id = %last_job_id,
                        "Skipping cron fire — previous job still active"
                    );
                    continue;
                }
            }
        }

        // Fire: submit a job (use template if available, else inline fields)
        let req = if let Some(tmpl_id) = cron.template_id {
            match claw_redis::get_job_template(pool, tmpl_id).await {
                Ok(Some(tmpl)) => CreateJobRequest {
                    prompt: tmpl.prompt,
                    skill_ids: tmpl.skill_ids,
                    skill_tags: vec![],
                    tool_ids: tmpl.tool_ids,
                    working_dir: None,
                    model: tmpl.model,
                    max_budget_usd: None,
                    allowed_tools: tmpl.allowed_tools,
                    output_dest: tmpl.output_dest,
                    tags: tmpl.tags,
                    priority: Some(tmpl.priority),
                    timeout_secs: tmpl.timeout_secs,
                    workspace_id: tmpl.workspace_id,
                    template_id: Some(tmpl_id),
                },
                _ => {
                    tracing::error!(cron_id = %cron.id, template_id = %tmpl_id, "Template not found for cron");
                    continue;
                }
            }
        } else {
            CreateJobRequest {
                prompt: cron.prompt.clone(),
                skill_ids: cron.skill_ids.clone(),
                skill_tags: vec![],
                tool_ids: cron.tool_ids.clone(),
                working_dir: Some(cron.working_dir.clone()),
                model: cron.model.clone(),
                max_budget_usd: cron.max_budget_usd,
                allowed_tools: None,
                output_dest: cron.output_dest.clone(),
                tags: cron.tags.clone(),
                priority: Some(cron.priority),
                timeout_secs: None,
                workspace_id: cron.workspace_id,
                template_id: None,
            }
        };

        match claw_redis::submit_job(pool, &req, JobSource::Cron).await {
            Ok(job) => {
                tracing::info!(
                    cron_id = %cron.id,
                    cron_name = %cron.name,
                    job_id = %job.id,
                    "Cron fired"
                );
                claw_redis::record_cron_fire(pool, cron.id, job.id).await.ok();
            }
            Err(e) => {
                tracing::error!(cron_id = %cron.id, error = %e, "Failed to submit cron job");
            }
        }
    }

    Ok(())
}
