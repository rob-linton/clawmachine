mod environment;
mod executor;
mod prompt_builder;

use deadpool_redis::{redis, Pool};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| "info,claw_worker=debug".into());
    if std::env::var("CLAW_LOG_FORMAT").as_deref() == Ok("json") {
        tracing_subscriber::fmt().json().with_env_filter(filter).init();
    } else {
        tracing_subscriber::fmt().with_env_filter(filter).init();
    }

    let redis_url = std::env::var("CLAW_REDIS_URL")
        .unwrap_or_else(|_| "redis://127.0.0.1:6379".into());
    let concurrency: usize = std::env::var("CLAW_WORKER_CONCURRENCY")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1);

    let pool = claw_redis::create_pool(&redis_url);
    let shutdown = Arc::new(AtomicBool::new(false));

    // Test connection
    {
        let mut conn = pool.get().await.expect("Failed to connect to Redis");
        let _: String = redis::cmd("PING")
            .query_async(&mut *conn)
            .await
            .expect("Redis PING failed");
    }

    // Crash recovery from previous unclean shutdowns
    environment::crash_recovery().await;

    let worker_id = format!(
        "{}-{}",
        hostname::get()
            .map(|h| h.to_string_lossy().into_owned())
            .unwrap_or_else(|_| "unknown".into()),
        std::process::id()
    );

    tracing::info!(worker_id, concurrency, "claw-worker starting");

    let mut handles = Vec::new();

    // Spawn worker tasks
    for task_idx in 0..concurrency {
        let pool = pool.clone();
        let shutdown = shutdown.clone();
        let task_id = format!("{}-task-{}", worker_id, task_idx);

        handles.push(tokio::spawn(async move {
            worker_loop(pool, task_id, shutdown).await;
        }));
    }

    // Spawn heartbeat task
    {
        let pool = pool.clone();
        let shutdown = shutdown.clone();
        let wid = worker_id.clone();
        handles.push(tokio::spawn(async move {
            heartbeat_loop(pool, wid, shutdown).await;
        }));
    }

    // Wait for Ctrl+C
    tokio::signal::ctrl_c().await.ok();
    tracing::info!("Shutting down...");
    shutdown.store(true, Ordering::Relaxed);

    for h in handles {
        h.await.ok();
    }

    // Clean up heartbeat
    claw_redis::delete_heartbeat(&pool, &worker_id).await.ok();
    tracing::info!("Worker stopped");
}

async fn heartbeat_loop(pool: Pool, worker_id: String, shutdown: Arc<AtomicBool>) {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(10));
    loop {
        interval.tick().await;
        if shutdown.load(Ordering::Relaxed) {
            break;
        }
        if let Err(e) = claw_redis::set_heartbeat(&pool, &worker_id, 30).await {
            tracing::warn!(error = %e, "Failed to refresh heartbeat");
        }
    }
}

async fn worker_loop(pool: Pool, task_id: String, shutdown: Arc<AtomicBool>) {
    tracing::info!(task_id, "Worker task started");

    loop {
        if shutdown.load(Ordering::Relaxed) {
            tracing::info!(task_id, "Shutdown signal received");
            break;
        }

        match claw_redis::claim_job(&pool, &task_id).await {
            Ok(Some(job)) => {
                let job_id = job.id;
                tracing::info!(
                    job_id = %job_id,
                    task_id,
                    prompt_len = job.prompt.len(),
                    "Job claimed, executing"
                );

                // 1. Resolve workspace (if workspace_id set)
                let workspace = if let Some(ws_id) = job.workspace_id {
                    match claw_redis::get_workspace(&pool, ws_id).await {
                        Ok(ws) => ws,
                        Err(e) => {
                            tracing::error!(job_id = %job_id, error = %e, "Failed to load workspace");
                            claw_redis::fail_job(&pool, job_id, &format!("Workspace load failed: {e}")).await.ok();
                            continue;
                        }
                    }
                } else {
                    None
                };

                // 2. Acquire workspace lock (if persistent workspace)
                if let Some(ws_id) = job.workspace_id {
                    let ttl = job.timeout_secs.unwrap_or(1800) + 60;
                    match claw_redis::acquire_workspace_lock(&pool, ws_id, job_id, ttl).await {
                        Ok(true) => {} // Lock acquired
                        Ok(false) => {
                            // Workspace busy — re-queue
                            claw_redis::requeue_job(&pool, job_id, job.priority).await.ok();
                            tracing::info!(job_id = %job_id, workspace_id = %ws_id, "Workspace locked, re-queued");
                            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                            continue;
                        }
                        Err(e) => {
                            tracing::error!(job_id = %job_id, error = %e, "Failed to acquire workspace lock");
                            claw_redis::fail_job(&pool, job_id, &format!("Workspace lock failed: {e}")).await.ok();
                            continue;
                        }
                    }
                }

                // 3. Resolve skills (workspace defaults + job-specific, deduplicated)
                let mut all_skill_ids = Vec::new();
                if let Some(ref ws) = workspace {
                    all_skill_ids.extend(ws.skill_ids.iter().cloned());
                }
                all_skill_ids.extend(job.skill_ids.iter().cloned());
                // Deduplicate
                let mut seen = std::collections::HashSet::new();
                all_skill_ids.retain(|id| seen.insert(id.clone()));

                let skills = claw_redis::resolve_skills(&pool, &all_skill_ids, &job.skill_tags)
                    .await
                    .unwrap_or_default();

                // 4. Prepare environment (workspace dir, CLAUDE.md, skill files)
                let prepared_env = match environment::prepare_environment(&job, workspace.as_ref(), &skills).await {
                    Ok(env) => env,
                    Err(e) => {
                        tracing::error!(job_id = %job_id, error = %e, "Failed to prepare environment");
                        claw_redis::fail_job(&pool, job_id, &format!("Environment setup failed: {e}")).await.ok();
                        if let Some(ws_id) = job.workspace_id {
                            claw_redis::release_workspace_lock(&pool, ws_id, job_id).await.ok();
                        }
                        continue;
                    }
                };

                // 5. Build prompt (templates only now — config/scripts are on disk)
                let built = prompt_builder::build_prompt(&job, &skills);
                let mut job = job;
                job.assembled_prompt = Some(built.prompt.clone());
                job.skill_snapshot = Some(built.skill_snapshot);
                claw_redis::update_job_fields(&pool, job_id, &job.skill_snapshot, &job.assembled_prompt).await.ok();

                // Publish running event
                let event = serde_json::json!({
                    "type": "job_update",
                    "job_id": job_id.to_string(),
                    "status": "running",
                    "worker_id": task_id,
                });
                claw_redis::publish_job_event(&pool, &event.to_string()).await.ok();

                // Set up log forwarding channel
                let (log_tx, mut log_rx) = mpsc::channel::<String>(256);
                let cancel = CancellationToken::new();

                let log_pool = pool.clone();
                let log_handle = tokio::spawn(async move {
                    while let Some(line) = log_rx.recv().await {
                        claw_redis::append_log(&log_pool, job_id, &line).await.ok();
                    }
                });

                let cancel_pool = pool.clone();
                let cancel_clone = cancel.clone();
                let cancel_handle = tokio::spawn(async move {
                    let mut interval = tokio::time::interval(std::time::Duration::from_secs(2));
                    loop {
                        interval.tick().await;
                        if let Ok(true) = claw_redis::is_cancelled(&cancel_pool, job_id).await {
                            cancel_clone.cancel();
                            break;
                        }
                    }
                });

                // 6. Execute in prepared workspace
                let result = executor::execute_job(&job, &prepared_env.working_dir, log_tx, cancel).await;

                // Stop helper tasks
                cancel_handle.abort();
                log_handle.await.ok();
                claw_redis::clear_cancel_flag(&pool, job_id).await.ok();

                // 7. Harvest new skills from workspace
                let harvested = environment::harvest_skills(&prepared_env).await;
                for new_skill in &harvested.new_skills {
                    tracing::info!(skill_id = %new_skill.id, job_id = %job_id, "Harvested new skill");
                    claw_redis::create_skill(&pool, new_skill).await.ok();
                }
                // Update workspace CLAUDE.md if modified
                if let (Some(ref modified_md), Some(ref ws)) = (&harvested.modified_claude_md, &workspace) {
                    let mut updated_ws = ws.clone();
                    updated_ws.claude_md = Some(modified_md.clone());
                    claw_redis::update_workspace(&pool, &updated_ws).await.ok();
                }

                // 8. Teardown environment
                environment::teardown_environment(&prepared_env).await;

                // 9. Release workspace lock
                if let Some(ws_id) = job.workspace_id {
                    claw_redis::release_workspace_lock(&pool, ws_id, job_id).await.ok();
                }

                // 10. Handle result
                match result {
                    Ok(r) => {
                        if let Err(e) = claw_redis::complete_job(
                            &pool, job_id, &r.result_text, r.cost_usd, r.duration_ms,
                        ).await {
                            tracing::error!(job_id = %job_id, error = %e, "Failed to store completion");
                        }

                        // File output
                        if let claw_models::OutputDest::File { path } = &job.output_dest {
                            if let Err(e) = tokio::fs::create_dir_all(path).await {
                                tracing::warn!(job_id = %job_id, error = %e, "Failed to create output dir");
                            } else {
                                let out_path = path.join(format!("{}.json", job_id));
                                let payload = serde_json::json!({
                                    "job_id": job_id.to_string(),
                                    "result": r.result_text,
                                    "cost_usd": r.cost_usd,
                                    "duration_ms": r.duration_ms,
                                    "completed_at": chrono::Utc::now().to_rfc3339(),
                                });
                                match tokio::fs::write(&out_path, serde_json::to_string_pretty(&payload).unwrap_or_default()).await {
                                    Ok(()) => tracing::info!(job_id = %job_id, path = %out_path.display(), "File output written"),
                                    Err(e) => tracing::warn!(job_id = %job_id, error = %e, "Failed to write file output"),
                                }
                            }
                        }

                        // Webhook output
                        if let claw_models::OutputDest::Webhook { url } = &job.output_dest {
                            let payload = serde_json::json!({
                                "job_id": job_id.to_string(),
                                "status": "completed",
                                "result": r.result_text,
                                "cost_usd": r.cost_usd,
                                "duration_ms": r.duration_ms,
                            });
                            match reqwest::Client::new()
                                .post(url)
                                .json(&payload)
                                .timeout(std::time::Duration::from_secs(30))
                                .send()
                                .await
                            {
                                Ok(resp) => tracing::info!(job_id = %job_id, status = %resp.status(), "Webhook delivered"),
                                Err(e) => tracing::error!(job_id = %job_id, error = %e, "Webhook delivery failed"),
                            }
                        }

                        let event = serde_json::json!({
                            "type": "job_update",
                            "job_id": job_id.to_string(),
                            "status": "completed",
                        });
                        claw_redis::publish_job_event(&pool, &event.to_string()).await.ok();
                    }
                    Err(e) => {
                        let status = if e == "Job was cancelled" { "cancelled" } else { "failed" };
                        if status == "cancelled" {
                            if let Err(re) = claw_redis::cancel_job(&pool, job_id).await {
                                tracing::error!(job_id = %job_id, error = %re, "Failed to store cancellation");
                            }
                        } else {
                            if let Err(re) = claw_redis::fail_job(&pool, job_id, &e).await {
                                tracing::error!(job_id = %job_id, error = %re, "Failed to store failure");
                            }
                            if let Ok(url) = std::env::var("CLAW_FAILURE_WEBHOOK_URL") {
                                let prompt_preview: String = job.prompt.chars().take(200).collect();
                                let payload = serde_json::json!({
                                    "job_id": job_id.to_string(),
                                    "prompt_preview": prompt_preview,
                                    "error": e,
                                    "source": format!("{:?}", job.source),
                                    "worker_id": task_id,
                                    "failed_at": chrono::Utc::now().to_rfc3339(),
                                });
                                match reqwest::Client::new()
                                    .post(&url)
                                    .json(&payload)
                                    .timeout(std::time::Duration::from_secs(10))
                                    .send()
                                    .await
                                {
                                    Ok(resp) => tracing::info!(job_id = %job_id, status = %resp.status(), "Failure webhook delivered"),
                                    Err(we) => tracing::error!(job_id = %job_id, error = %we, "Failure webhook failed"),
                                }
                            }
                        }
                        let event = serde_json::json!({
                            "type": "job_update",
                            "job_id": job_id.to_string(),
                            "status": status,
                        });
                        claw_redis::publish_job_event(&pool, &event.to_string()).await.ok();
                    }
                }
            }
            Ok(None) => {
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            }
            Err(e) => {
                tracing::error!(task_id, error = %e, "Failed to claim job");
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            }
        }
    }
}
