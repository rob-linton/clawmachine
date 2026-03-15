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
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,claw_worker=debug".into()),
        )
        .init();

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

                // Build prompt with skill injection
                let built = prompt_builder::build_prompt(&pool, &job).await;
                let mut job = job;
                job.assembled_prompt = Some(built.prompt.clone());
                job.skill_snapshot = Some(built.skill_snapshot);

                // Persist the skill snapshot to Redis
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

                // Log forwarder task
                let log_pool = pool.clone();
                let log_handle = tokio::spawn(async move {
                    while let Some(line) = log_rx.recv().await {
                        claw_redis::append_log(&log_pool, job_id, &line).await.ok();
                    }
                });

                // Cancellation watcher task
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

                // Execute
                let result = executor::execute_job(&job, log_tx, cancel).await;

                // Stop helper tasks
                cancel_handle.abort();
                log_handle.await.ok();

                // Clean up cancel flag
                claw_redis::clear_cancel_flag(&pool, job_id).await.ok();

                match result {
                    Ok(r) => {
                        if let Err(e) = claw_redis::complete_job(
                            &pool, job_id, &r.result_text, r.cost_usd, r.duration_ms,
                        ).await {
                            tracing::error!(job_id = %job_id, error = %e, "Failed to store completion");
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
                        } else if let Err(re) = claw_redis::fail_job(&pool, job_id, &e).await {
                            tracing::error!(job_id = %job_id, error = %re, "Failed to store failure");
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
