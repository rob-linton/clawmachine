mod docker;
mod environment;
mod executor;
mod pipeline_runner;
mod prompt_builder;

use claw_models::WorkspacePersistence;
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

    // Verify Claude CLI is available
    match tokio::process::Command::new("claude").arg("--version").output().await {
        Ok(output) if output.status.success() => {
            let version = String::from_utf8_lossy(&output.stdout);
            tracing::info!(version = %version.trim(), "Claude CLI verified");
        }
        Ok(output) => {
            tracing::error!(
                stderr = %String::from_utf8_lossy(&output.stderr),
                "Claude CLI returned error. Is it installed and authenticated?"
            );
            std::process::exit(1);
        }
        Err(e) => {
            tracing::error!(error = %e, "Claude CLI not found. Install it: https://claude.ai/code");
            std::process::exit(1);
        }
    }

    // Crash recovery: clean up stale temp dirs from previous runs
    environment::crash_recovery().await;
    // Also clean up stale job/pipeline temp dirs
    for prefix in &["/tmp/claw-job-", "/tmp/claw-pipeline-"] {
        if let Ok(mut entries) = tokio::fs::read_dir("/tmp").await {
            while let Ok(Some(entry)) = entries.next_entry().await {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.starts_with(prefix.trim_start_matches("/tmp/")) {
                    tracing::info!(path = %entry.path().display(), "Cleaning up stale temp dir");
                    tokio::fs::remove_dir_all(entry.path()).await.ok();
                }
            }
        }
    }

    // If Docker backend is configured, ensure sandbox image is available
    let backend_str = claw_redis::get_config(&pool, "execution_backend")
        .await
        .unwrap_or_else(|_| {
            std::env::var("CLAW_EXECUTION_BACKEND").unwrap_or_else(|_| "local".into())
        });
    if backend_str == "docker" {
        let image = claw_redis::get_config(&pool, "sandbox_image")
            .await
            .unwrap_or_else(|_| "claw-sandbox:latest".into());
        match docker::ensure_image(&image).await {
            Ok(()) => tracing::info!(image, "Docker sandbox image ready"),
            Err(e) => tracing::warn!(error = %e, "Docker sandbox image not available — Docker jobs will fail until image is built/pulled"),
        }
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

    // Spawn reaper task (only on task-0 worker to avoid duplicate reaping)
    {
        let pool = pool.clone();
        let shutdown = shutdown.clone();
        handles.push(tokio::spawn(async move {
            reaper_loop(pool, shutdown).await;
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

async fn reaper_loop(pool: Pool, shutdown: Arc<AtomicBool>) {
    // Wait a bit before first reap to allow workers to start up
    tokio::time::sleep(std::time::Duration::from_secs(30)).await;
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
    loop {
        interval.tick().await;
        if shutdown.load(Ordering::Relaxed) {
            break;
        }
        match claw_redis::reap_dead_workers(&pool).await {
            Ok(0) => {}
            Ok(n) => tracing::info!(reaped = n, "Reaper recovered jobs from dead workers"),
            Err(e) => tracing::warn!(error = %e, "Reaper check failed"),
        }
    }
}

async fn worker_loop(pool: Pool, task_id: String, shutdown: Arc<AtomicBool>) {
    tracing::info!(task_id, "Worker task started");

    // Pipeline checkout reuse: shared map across all jobs in this worker task
    let pipeline_checkouts: environment::PipelineCheckouts =
        Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new()));

    loop {
        if shutdown.load(Ordering::Relaxed) {
            tracing::info!(task_id, "Shutdown signal received");
            break;
        }

        // Re-read execution backend config each iteration (so Settings changes take effect)
        let backend_str = claw_redis::get_config(&pool, "execution_backend")
            .await
            .unwrap_or_else(|_| {
                std::env::var("CLAW_EXECUTION_BACKEND").unwrap_or_else(|_| "local".into())
            });
        let backend = executor::ExecutionBackend::from_config_str(&backend_str);
        let docker_config = if backend == executor::ExecutionBackend::Docker {
            let all_config = claw_redis::get_all_config(&pool).await.unwrap_or_default();
            Some(docker::DockerConfig::from_config(&all_config))
        } else {
            None
        };

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

                // 2. Acquire workspace lock (if persistent workspace, and not part of a pipeline)
                // Pipeline jobs skip per-job locking — the pipeline runner manages the lock
                let is_pipeline_job = job.pipeline_run_id.is_some();
                if !is_pipeline_job {
                    if let Some(ws_id) = job.workspace_id {
                        let ttl = job.timeout_secs.unwrap_or(1800) + 60;
                        match claw_redis::acquire_workspace_lock(&pool, ws_id, job_id, ttl).await {
                            Ok(true) => {} // Lock acquired
                            Ok(false) => {
                                // Workspace busy — re-queue with time-based limit
                                let elapsed = chrono::Utc::now() - job.created_at;
                                let max_wait = chrono::Duration::hours(1);
                                if elapsed > max_wait {
                                    claw_redis::fail_job(&pool, job_id, &format!(
                                        "Workspace {} locked for over 1 hour, giving up",
                                        ws_id
                                    )).await.ok();
                                    tracing::warn!(job_id = %job_id, workspace_id = %ws_id, "Job failed: workspace contention timeout");
                                    continue;
                                }
                                claw_redis::requeue_job(&pool, job_id, job.priority).await.ok();
                                let elapsed_secs = elapsed.num_seconds().max(0) as u64;
                                tracing::info!(job_id = %job_id, workspace_id = %ws_id, elapsed_secs, "Workspace locked, re-queued");
                                // Backoff: 5s base, scaling with elapsed time, capped at 60s
                                let backoff = std::cmp::min(5 + (elapsed_secs / 30), 60);
                                tokio::time::sleep(std::time::Duration::from_secs(backoff)).await;
                                continue;
                            }
                            Err(e) => {
                                tracing::error!(job_id = %job_id, error = %e, "Failed to acquire workspace lock");
                                claw_redis::fail_job(&pool, job_id, &format!("Workspace lock failed: {e}")).await.ok();
                                continue;
                            }
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

                // 3b. Git snapshot: pre-job commit (before skills deployed)
                // Only for legacy workspaces — new workspaces get fresh clones
                if let Some(ref ws) = workspace {
                    if ws.is_legacy() {
                        let ws_path = ws.path.clone().unwrap();
                        let jid = job_id.to_string();
                        tokio::task::spawn_blocking(move || {
                            git_commit(&ws_path, &format!("claw: pre-job {}", jid));
                        }).await.ok();
                    }
                }

                // 4. Prepare environment (workspace dir, CLAUDE.md, skill files)
                let prepared_env = match environment::prepare_environment(&job, workspace.as_ref(), &skills, &pipeline_checkouts).await {
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
                let result = executor::dispatch_execute(
                    &backend,
                    &job,
                    &prepared_env.working_dir,
                    docker_config.as_ref(),
                    log_tx,
                    cancel,
                ).await;

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

                // 8b. Git snapshot: post-job commit
                // Legacy: commit in workspace dir. New: commit in temp checkout + push to bare repo.
                if let Some(ref ws) = workspace {
                    if ws.is_legacy() && !prepared_env.is_temp {
                        let ws_path = ws.path.clone().unwrap();
                        let jid = job_id.to_string();
                        tokio::task::spawn_blocking(move || {
                            git_commit(&ws_path, &format!("claw: post-job {}", jid));
                        }).await.ok();
                    } else if !ws.is_legacy() && ws.persistence == WorkspacePersistence::Persistent {
                        // New-style persistent: commit and push from temp checkout to bare repo
                        let working = prepared_env.working_dir.clone();
                        let jid = job_id.to_string();
                        tokio::task::spawn_blocking(move || {
                            git_commit(&working, &format!("claw: post-job {}", jid));
                            std::process::Command::new("git")
                                .args(["push", "origin", "HEAD"])
                                .current_dir(&working)
                                .stdout(std::process::Stdio::null())
                                .stderr(std::process::Stdio::null())
                                .status()
                                .ok();
                        }).await.ok();
                    } else if !ws.is_legacy() && ws.persistence == WorkspacePersistence::Snapshot {
                        // Snapshot: commit and push to snapshot branch for inspection
                        let working = prepared_env.working_dir.clone();
                        let jid = job_id.to_string();
                        tokio::task::spawn_blocking(move || {
                            git_commit(&working, &format!("claw: post-job {}", jid));
                            // Push the job branch to bare repo as a snapshot ref
                            let branch = format!("claw/snapshot-{}", jid);
                            std::process::Command::new("git")
                                .args(["push", "origin", &format!("HEAD:refs/heads/{}", branch)])
                                .current_dir(&working)
                                .stdout(std::process::Stdio::null())
                                .stderr(std::process::Stdio::null())
                                .status()
                                .ok();
                        }).await.ok();
                    }
                    // Ephemeral: no commit, no push — temp dir just gets cleaned up
                }

                // 9. Release workspace lock (skip for pipeline jobs — pipeline runner handles it)
                if !is_pipeline_job {
                    if let Some(ws_id) = job.workspace_id {
                        claw_redis::release_workspace_lock(&pool, ws_id, job_id).await.ok();
                    }
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

                        // Global completion webhook
                        if let Ok(url) = std::env::var("CLAW_COMPLETION_WEBHOOK_URL") {
                            let payload = serde_json::json!({
                                "job_id": job_id.to_string(),
                                "status": "completed",
                                "result_preview": r.result_text.chars().take(500).collect::<String>(),
                                "cost_usd": r.cost_usd,
                                "duration_ms": r.duration_ms,
                            });
                            reqwest::Client::new()
                                .post(&url)
                                .json(&payload)
                                .timeout(std::time::Duration::from_secs(10))
                                .send()
                                .await
                                .ok();
                        }

                        // Advance pipeline if this job is part of one
                        pipeline_runner::check_and_advance(&pool, &job, &r.result_text).await;
                    }
                    Err(e) => {
                        if e == "Job was cancelled" {
                            if let Err(re) = claw_redis::cancel_job(&pool, job_id).await {
                                tracing::error!(job_id = %job_id, error = %re, "Failed to store cancellation");
                            }
                            let event = serde_json::json!({
                                "type": "job_update",
                                "job_id": job_id.to_string(),
                                "status": "cancelled",
                            });
                            claw_redis::publish_job_event(&pool, &event.to_string()).await.ok();
                        } else {
                            // fail_job returns Ok(true) if job was re-queued for retry
                            let was_retried = match claw_redis::fail_job(&pool, job_id, &e).await {
                                Ok(retried) => retried,
                                Err(re) => {
                                    tracing::error!(job_id = %job_id, error = %re, "Failed to store failure");
                                    false
                                }
                            };

                            if was_retried {
                                // Job re-queued — publish retry event, skip failure webhook
                                let event = serde_json::json!({
                                    "type": "job_update",
                                    "job_id": job_id.to_string(),
                                    "status": "pending",
                                });
                                claw_redis::publish_job_event(&pool, &event.to_string()).await.ok();
                            } else {
                                // Terminal failure
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
                                let event = serde_json::json!({
                                    "type": "job_update",
                                    "job_id": job_id.to_string(),
                                    "status": "failed",
                                });
                                claw_redis::publish_job_event(&pool, &event.to_string()).await.ok();

                                // Mark pipeline as failed if this job is part of one
                                pipeline_runner::mark_failed(&pool, &job, &e).await;
                            }
                        }
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

/// Git add + commit in a workspace (best effort, skips if nothing to commit)
fn git_commit(path: &std::path::Path, message: &str) {
    if !path.join(".git").exists() {
        return;
    }

    let run = |args: &[&str]| -> bool {
        std::process::Command::new("git")
            .args(args)
            .current_dir(path)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    };

    run(&["add", "-A"]);
    run(&["-c", "user.name=ClaudeCodeClaw", "-c", "user.email=claw@local", "commit", "-m", message]);
}
