mod docker;
mod environment;
mod executor;
mod pipeline_runner;
mod prompt_builder;
mod secrets;
pub mod session_container;
pub mod summarizer;
mod token_refresh;

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

    // Crash recovery: clean up stale job/pipeline dirs from previous runs
    environment::crash_recovery().await;

    // Execution backend: env var takes priority (set by docker-compose), then Redis config
    let backend_str = match std::env::var("CLAW_EXECUTION_BACKEND") {
        Ok(v) if !v.is_empty() => v,
        _ => claw_redis::get_config(&pool, "execution_backend")
            .await
            .unwrap_or_else(|_| "docker".into()),
    };
    if backend_str == "docker" {
        // Check Docker socket is accessible
        if let Err(e) = docker::check_docker_socket().await {
            tracing::error!(error = %e, "Docker socket not accessible — cannot start in Docker mode");
            tracing::info!("Mount /var/run/docker.sock into the worker container, or set execution_backend=local");
            std::process::exit(1);
        }

        // Check sandbox image exists
        let image = claw_redis::get_config(&pool, "sandbox_image")
            .await
            .unwrap_or_else(|_| "claw-sandbox:latest".into());
        match docker::ensure_image(&image).await {
            Ok(()) => tracing::info!(image, "Docker sandbox image ready"),
            Err(e) => {
                tracing::error!(error = %e, "Docker sandbox image not available — cannot start in Docker mode");
                tracing::info!("Build with: POST /api/v1/docker/images/build, or set execution_backend=local");
                std::process::exit(1);
            }
        }

        // Validate host data dir if set (for Docker-in-Docker volume mapping)
        if let Ok(host_dir) = std::env::var("CLAW_HOST_DATA_DIR") {
            tracing::info!(host_data_dir = %host_dir, "Docker-in-Docker host path mapping configured");
        }
    }

    let worker_id = format!(
        "{}-{}",
        hostname::get()
            .map(|h| h.to_string_lossy().into_owned())
            .unwrap_or_else(|_| "unknown".into()),
        std::process::id()
    );

    // Startup OAuth token check
    {
        let api_key = claw_redis::get_config(&pool, "anthropic_api_key")
            .await
            .ok()
            .filter(|s| !s.is_empty())
            .or_else(|| std::env::var("ANTHROPIC_API_KEY").ok().filter(|s| !s.is_empty()));
        match token_refresh::ensure_token_fresh(api_key.as_deref()).await {
            Ok(true) => tracing::info!("OAuth token refreshed at startup"),
            Ok(false) => {}
            Err(e) => tracing::error!(error = %e, "Startup token refresh failed"),
        }
        token_refresh::write_oauth_status(&pool).await;
    }

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

    // Spawn token refresh task (hourly OAuth token refresh)
    {
        let pool = pool.clone();
        let shutdown = shutdown.clone();
        handles.push(tokio::spawn(async move {
            token_refresh_loop(pool, shutdown).await;
        }));
    }

    // Clean up orphaned session containers on startup
    {
        let pool = pool.clone();
        session_container::cleanup_orphans(&pool).await;
    }

    // Spawn session container idle cleanup task (every 60s)
    {
        let pool = pool.clone();
        let shutdown = shutdown.clone();
        handles.push(tokio::spawn(async move {
            let timeout_secs = 1800; // 30 minutes
            loop {
                if shutdown.load(std::sync::atomic::Ordering::Relaxed) { break; }
                tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                session_container::cleanup_idle_containers(&pool, timeout_secs).await;
            }
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

async fn token_refresh_loop(pool: Pool, shutdown: Arc<AtomicBool>) {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(3600));
    loop {
        interval.tick().await;
        if shutdown.load(Ordering::Relaxed) {
            break;
        }
        let api_key = claw_redis::get_config(&pool, "anthropic_api_key")
            .await
            .ok()
            .filter(|s| !s.is_empty())
            .or_else(|| std::env::var("ANTHROPIC_API_KEY").ok().filter(|s| !s.is_empty()));
        match token_refresh::ensure_token_fresh(api_key.as_deref()).await {
            Ok(true) => tracing::info!("OAuth token refreshed proactively"),
            Ok(false) => {}
            Err(e) => tracing::error!(error = %e, "Periodic token refresh failed"),
        }
        token_refresh::write_oauth_status(&pool).await;
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

        // Re-read execution backend each iteration. Env var (from compose) takes priority,
        // then Redis config (from Settings screen), then default.
        let backend_str = match std::env::var("CLAW_EXECUTION_BACKEND") {
            Ok(v) if !v.is_empty() => v,
            _ => claw_redis::get_config(&pool, "execution_backend")
                .await
                .unwrap_or_else(|_| "local".into()),
        };
        let backend = executor::ExecutionBackend::from_config_str(&backend_str);

        // Resolve auth: prefer OAuth over API key.
        // If OAuth is valid, don't pass API key (hide it from Claude Code).
        // If OAuth is expired/missing, fall back to API key.
        let raw_api_key = claw_redis::get_config(&pool, "anthropic_api_key")
            .await
            .ok()
            .filter(|s| !s.is_empty())
            .or_else(|| std::env::var("ANTHROPIC_API_KEY").ok().filter(|s| !s.is_empty()));
        let anthropic_api_key = if token_refresh::is_oauth_valid() {
            None // OAuth is valid — hide API key from Claude Code
        } else {
            raw_api_key.clone() // OAuth unavailable — fall back to API key
        };

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

                // Pre-job OAuth token check
                // For chat jobs: always try OAuth refresh (session containers use OAuth directly)
                // For regular jobs: skip refresh if API key is available
                let is_chat_job = job.tags.iter().any(|t| t.starts_with("chat:"));
                let refresh_key = if is_chat_job { None } else { raw_api_key.as_deref() };
                if let Err(e) = token_refresh::ensure_token_fresh(refresh_key).await {
                    tracing::warn!(job_id = %job_id, error = %e, "Token refresh failed — job may fail if expired");
                }

                // CHAT FAST PATH: route chat jobs through session container
                // Tasks (tagged "task") skip this — they use the standard job path for parallelism
                let is_task = job.tags.iter().any(|t| t == "task");
                if !is_task {
                if let Some(chat_tag) = job.tags.iter().find(|t| t.starts_with("chat:")) {
                    if let Ok(chat_id) = chat_tag.strip_prefix("chat:").unwrap_or("").parse::<uuid::Uuid>() {
                        if backend == executor::ExecutionBackend::Docker {
                            if let Some(ref dc) = docker_config {
                                let ws_id = job.workspace_id.unwrap_or_default();
                                let seq_tag = job.tags.iter().find(|t| t.starts_with("chat_seq:"));
                                let seq: u32 = seq_tag.and_then(|t| t.strip_prefix("chat_seq:")).and_then(|s| s.parse().ok()).unwrap_or(1);

                                // Acquire per-chat execution lock — only one message at a time
                                // (--continue requires sequential execution)
                                let lock_ttl = job.timeout_secs.unwrap_or(600) + 30;
                                match claw_redis::try_acquire_chat_lock(&pool, chat_id, job_id, lock_ttl).await {
                                    Ok(false) => {
                                        // Another message is executing — re-queue with short backoff
                                        tracing::info!(job_id = %job_id, chat_id = %chat_id, "Chat lock held, re-queuing");
                                        claw_redis::requeue_chat_job(&pool, job_id).await.ok();
                                        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                                        continue;
                                    }
                                    Err(e) => {
                                        tracing::error!(job_id = %job_id, error = %e, "Chat lock acquisition failed");
                                        claw_redis::fail_job(&pool, job_id, &format!("Lock error: {e}")).await.ok();
                                        continue;
                                    }
                                    Ok(true) => {} // Lock acquired, proceed
                                }

                                tracing::info!(job_id = %job_id, chat_id = %chat_id, seq, "Chat via session container");

                                // Get username for notebook operations
                                let chat_username = claw_redis::get_chat_session(&pool, chat_id).await
                                    .ok().flatten().map(|s| s.user_id.clone()).unwrap_or_default();

                                // Resolve workspace, tools, and credentials for the chat container
                                let chat_workspace = claw_redis::get_workspace(&pool, ws_id).await.ok().flatten();
                                let mut chat_tool_ids = chat_workspace.as_ref().map(|w| w.tool_ids.clone()).unwrap_or_default();
                                // Include session-level tools (added via chat install requests)
                                if let Ok(Some(sess)) = claw_redis::get_chat_session(&pool, chat_id).await {
                                    for tid in &sess.tool_ids {
                                        if !chat_tool_ids.contains(tid) { chat_tool_ids.push(tid.clone()); }
                                    }
                                }
                                let chat_tools: Vec<claw_models::Tool> = if chat_tool_ids.is_empty() {
                                    Vec::new()
                                } else {
                                    claw_redis::resolve_tools(&pool, &chat_tool_ids).await.unwrap_or_default()
                                };

                                // Resolve credential env vars for tools
                                let mut chat_credential_env = std::collections::HashMap::new();
                                if let Some(ref ws) = chat_workspace {
                                    for tool in &chat_tools {
                                        if let Some(cred_id) = ws.credential_bindings.get(&tool.id) {
                                            if let Ok(Some(values)) = claw_redis::get_credential_values(&pool, cred_id).await {
                                                chat_credential_env.extend(values);
                                            }
                                        }
                                    }
                                }

                                match session_container::ensure_container(&pool, chat_id, ws_id, dc, raw_api_key.as_deref(), &chat_tools, chat_workspace.as_ref()).await {
                                    Ok((container_name, is_new_container)) => {
                                        // is_first: use container freshness, not seq == 1.
                                        // A new container has no --continue history to resume.
                                        let is_first = is_new_container;
                                        let needs_rehydration = is_new_container && seq > 1;

                                        if needs_rehydration {
                                            tracing::info!(chat_id = %chat_id, seq, "Rehydrating after container restart");
                                        }

                                        // --- PRE-EXEC: prepare workspace ---

                                        // Deploy notebook from Redis to workspace
                                        session_container::deploy_notebook(&pool, &chat_username, ws_id).await.ok();

                                        // Deploy dynamic CLAUDE.md (temporal context + user profile + memories)
                                        session_container::deploy_dynamic_claude_md(&pool, chat_id, ws_id, &chat_username).await.ok();

                                        // Refresh available skills/tools + deploy API skill
                                        session_container::refresh_available_catalog(&pool, ws_id).await;
                                        session_container::deploy_api_skill(ws_id).await;

                                        // Read raw user message from file (API writes it before job submission).
                                        let user_msg_path = dirs::home_dir().unwrap_or_else(|| "/tmp".into())
                                            .join(".claw/checkouts").join(ws_id.to_string())
                                            .join(".chat/messages").join(format!("{:04}-user.md", seq));
                                        let user_message = tokio::fs::read_to_string(&user_msg_path).await
                                            .unwrap_or_else(|_| job.prompt.clone());

                                        // Build effective message (rehydration wraps user message with context)
                                        let effective_message = if needs_rehydration {
                                            session_container::build_rehydration_prompt(
                                                &pool, chat_id, ws_id, &chat_username, &user_message
                                            ).await
                                        } else {
                                            user_message.clone()
                                        };

                                        // --- EXECUTE with cancellation support ---
                                        // Forward stream-json log lines to Redis so the chat-message
                                        // job ends up with a proper log accessible via
                                        // GET /api/v1/jobs/{id}/logs. This is what the chat UI's
                                        // "Show full" tool-result button reads back, and it also
                                        // populates the /jobs/{id} activity panel for chat jobs.
                                        let (log_tx, mut log_rx) = tokio::sync::mpsc::channel::<String>(256);
                                        let log_pool = pool.clone();
                                        let log_job_id = job_id;
                                        let log_handle = tokio::spawn(async move {
                                            while let Some(line) = log_rx.recv().await {
                                                claw_redis::append_log(&log_pool, log_job_id, &line).await.ok();
                                            }
                                        });
                                        let chat_cancel = CancellationToken::new();
                                        let chat_cancel_clone = chat_cancel.clone();
                                        let chat_cancel_pool = pool.clone();
                                        let chat_cancel_job_id = job_id;
                                        let chat_cancel_handle = tokio::spawn(async move {
                                            let mut interval = tokio::time::interval(std::time::Duration::from_secs(2));
                                            loop {
                                                interval.tick().await;
                                                if let Ok(true) = claw_redis::is_cancelled(&chat_cancel_pool, chat_cancel_job_id).await {
                                                    chat_cancel_clone.cancel();
                                                    break;
                                                }
                                            }
                                        });

                                        match session_container::execute_chat_message(
                                            &pool, chat_id, &container_name, ws_id, &effective_message, job.model.as_deref(), is_first, seq, log_tx, chat_cancel,
                                            &chat_credential_env, &chat_tools,
                                        ).await {
                                            Ok(r) => {
                                                chat_cancel_handle.abort();
                                                claw_redis::clear_cancel_flag(&pool, job_id).await.ok();

                                                // --- POST-EXEC: store results ---
                                                claw_redis::complete_job(&pool, job_id, &r.result_text, r.cost_usd, r.duration_ms).await.ok();

                                                // Extract artifacts before saving message so IDs are included
                                                let artifact_ids = session_container::extract_artifacts(ws_id, seq, &r.result_text).await;

                                                if seq > 0 {
                                                    let msg = claw_models::ChatMessage {
                                                        seq, role: "assistant".to_string(), content: r.result_text.clone(),
                                                        summary: None, job_id: Some(job_id), cost_usd: Some(r.cost_usd),
                                                        model: job.model.clone(), token_estimate: (r.result_text.len() / 4).max(1) as u32,
                                                        files_written: r.files_written.clone(), artifacts: artifact_ids,
                                                        thinking: r.thinking.clone(),
                                                        status: "complete".to_string(), timestamp: chrono::Utc::now(),
                                                    };
                                                    claw_redis::add_chat_message(&pool, chat_id, &msg).await.ok();
                                                    let msgs_dir = dirs::home_dir().unwrap_or_else(|| "/tmp".into())
                                                        .join(".claw/checkouts").join(ws_id.to_string()).join(".chat/messages");
                                                    tokio::fs::create_dir_all(&msgs_dir).await.ok();
                                                    tokio::fs::write(msgs_dir.join(format!("{:04}-assistant.md", seq)), &r.result_text).await.ok();
                                                    if let Ok(Some(mut session)) = claw_redis::get_chat_session(&pool, chat_id).await {
                                                        session.total_messages += 1;
                                                        session.total_cost_usd += r.cost_usd;
                                                        session.last_activity = chrono::Utc::now();
                                                        session.updated_at = chrono::Utc::now();
                                                        claw_redis::update_chat_session(&pool, &session).await.ok();
                                                    }
                                                }
                                                tracing::info!(job_id = %job_id, duration_ms = r.duration_ms, "Chat completed");

                                                // Harvest notebook changes Claude made during the message
                                                let changed_files = session_container::harvest_notebook(&pool, &chat_username, ws_id, seq).await
                                                    .unwrap_or_default();

                                                // Release lock (after harvest, before background work)
                                                claw_redis::release_chat_lock(&pool, chat_id, job_id).await.ok();
                                                session_container::process_install_requests(&pool, ws_id, chat_id).await;

                                                // Periodic git commit every 5 messages
                                                if seq > 0 && seq % 5 == 0 {
                                                    let checkout = dirs::home_dir().unwrap_or_else(|| "/tmp".into())
                                                        .join(".claw/checkouts").join(ws_id.to_string());
                                                    session_container::git_commit(&checkout, &format!("chat: auto-commit at message {}", seq)).await;
                                                }

                                                // Publish SSE event so the UI gets notified
                                                let event = serde_json::json!({
                                                    "type": "job_update",
                                                    "job_id": job_id.to_string(),
                                                    "status": "completed",
                                                });
                                                claw_redis::publish_job_event(&pool, &event.to_string()).await.ok();

                                                // --- BACKGROUND: cognitive pipeline ---
                                                let pool_bg = pool.clone();
                                                let dc_bg = dc.clone();
                                                let user_content_bg = user_message.clone();
                                                let result_text_bg = r.result_text.clone();
                                                let username_bg = chat_username.clone();
                                                let changed_bg = changed_files;
                                                tokio::spawn(async move {
                                                    let container = match summarizer::ensure_summarizer_container(&dc_bg).await {
                                                        Ok(c) => c,
                                                        Err(e) => { tracing::warn!("Summarizer container failed: {e}"); return; }
                                                    };

                                                    let notebook_index = claw_redis::memory::build_notebook_index(&pool_bg, &username_bg).await.unwrap_or_default();
                                                    let recent_moods = claw_redis::memory::get_recent_moods(&pool_bg, &username_bg, 5).await.unwrap_or_default();

                                                    if let Some(result) = summarizer::run_cognitive_pipeline(
                                                        &container, &user_content_bg, &result_text_bg,
                                                        &notebook_index, &changed_bg, &recent_moods, seq,
                                                    ).await {
                                                        // Store message summary
                                                        claw_redis::update_message_summary(&pool_bg, chat_id, seq, "assistant", &result.summary).await.ok();

                                                        // Apply notebook operations
                                                        let now = chrono::Utc::now();
                                                        for op in &result.notebook_ops {
                                                            match op.op.as_str() {
                                                                "create" | "update" => {
                                                                    let entry = claw_redis::memory::NotebookEntry {
                                                                        content: op.content.clone(),
                                                                        summary: op.summary.clone(),
                                                                        created: claw_redis::memory::get_notebook_entry(&pool_bg, &username_bg, &op.file).await
                                                                            .ok().flatten().map(|e| e.created).unwrap_or(now),
                                                                        updated: now,
                                                                        access_count: 0,
                                                                        last_accessed: now,
                                                                    };
                                                                    claw_redis::memory::upsert_notebook_entry(&pool_bg, &username_bg, &op.file, &entry).await.ok();
                                                                }
                                                                "append" => {
                                                                    // Append to existing entry
                                                                    if let Ok(Some(mut existing)) = claw_redis::memory::get_notebook_entry(&pool_bg, &username_bg, &op.file).await {
                                                                        existing.content.push('\n');
                                                                        existing.content.push_str(&op.content);
                                                                        existing.updated = now;
                                                                        existing.summary = op.summary.clone();
                                                                        claw_redis::memory::upsert_notebook_entry(&pool_bg, &username_bg, &op.file, &existing).await.ok();
                                                                    } else {
                                                                        // File doesn't exist yet — create it
                                                                        let entry = claw_redis::memory::NotebookEntry {
                                                                            content: op.content.clone(),
                                                                            summary: op.summary.clone(),
                                                                            created: now, updated: now,
                                                                            access_count: 0, last_accessed: now,
                                                                        };
                                                                        claw_redis::memory::upsert_notebook_entry(&pool_bg, &username_bg, &op.file, &entry).await.ok();
                                                                    }
                                                                }
                                                                "delete" => {
                                                                    claw_redis::memory::delete_notebook_entry(&pool_bg, &username_bg, &op.file).await.ok();
                                                                }
                                                                _ => {}
                                                            }
                                                        }

                                                        // Store mood
                                                        if let Some(ref mood) = result.mood {
                                                            claw_redis::memory::append_mood(&pool_bg, &username_bg, mood).await.ok();
                                                        }

                                                        // Store anticipation
                                                        if let Some(ref anticipation) = result.anticipation {
                                                            claw_redis::memory::update_anticipation(&pool_bg, &username_bg, anticipation).await.ok();
                                                        }

                                                        // Rolling summary every 10 messages
                                                        if seq % 10 == 0 && seq > 0 {
                                                            summarizer::update_rolling_summary(&container, &pool_bg, chat_id, ws_id).await.ok();
                                                        }

                                                        // Session digest every 30 messages
                                                        if seq % 30 == 0 && seq > 0 {
                                                            summarizer::generate_session_digest(&pool_bg, &username_bg, chat_id, &container).await.ok();
                                                        }
                                                    }
                                                });
                                            }
                                            Err(e) => {
                                                chat_cancel_handle.abort();
                                                claw_redis::clear_cancel_flag(&pool, job_id).await.ok();

                                                let is_cancelled = e.contains("cancelled");
                                                if is_cancelled {
                                                    tracing::info!(job_id = %job_id, "Chat message cancelled");
                                                } else {
                                                    tracing::error!(job_id = %job_id, error = %e, "Chat execution failed");
                                                }

                                                // Harvest notebook + git commit even on cancel (preserve partial work)
                                                session_container::harvest_notebook(&pool, &chat_username, ws_id, seq).await.ok();
                                                let checkout = dirs::home_dir().unwrap_or_else(|| "/tmp".into())
                                                    .join(".claw/checkouts").join(ws_id.to_string());
                                                session_container::git_commit(&checkout, &format!("chat: {} at message {}", if is_cancelled { "cancelled" } else { "failed" }, seq)).await;

                                                // Release lock before anything else
                                                claw_redis::release_chat_lock(&pool, chat_id, job_id).await.ok();

                                                if is_cancelled {
                                                    // Cancel: mark job cancelled, keep container alive
                                                    claw_redis::cancel_job(&pool, job_id).await.ok();
                                                    if seq > 0 {
                                                        let err_msg = claw_models::ChatMessage {
                                                            seq, role: "assistant".to_string(),
                                                            content: "[Cancelled]".to_string(),
                                                            summary: None, job_id: Some(job_id), cost_usd: None,
                                                            model: None, token_estimate: 0, thinking: None,
                                                            files_written: Vec::new(), artifacts: Vec::new(), status: "complete".to_string(), timestamp: chrono::Utc::now(),
                                                        };
                                                        claw_redis::add_chat_message(&pool, chat_id, &err_msg).await.ok();
                                                    }
                                                    let event = serde_json::json!({"type": "job_update", "job_id": job_id.to_string(), "status": "cancelled"});
                                                    claw_redis::publish_job_event(&pool, &event.to_string()).await.ok();
                                                } else {
                                                    // Real error: delete container, mark failed
                                                    let is_terminal = claw_redis::fail_job(&pool, job_id, &e).await.ok() == Some(false);
                                                    claw_redis::delete_chat_container(&pool, chat_id).await.ok();
                                                    if is_terminal && seq > 0 {
                                                        let err_msg = claw_models::ChatMessage {
                                                            seq, role: "assistant".to_string(),
                                                            content: format!("Error: {}", e),
                                                            summary: None, job_id: Some(job_id), cost_usd: None,
                                                            model: None, token_estimate: 0, thinking: None,
                                                            files_written: Vec::new(), artifacts: Vec::new(), status: "complete".to_string(), timestamp: chrono::Utc::now(),
                                                        };
                                                        claw_redis::add_chat_message(&pool, chat_id, &err_msg).await.ok();
                                                    }
                                                    let event = serde_json::json!({"type": "job_update", "job_id": job_id.to_string(), "status": "failed"});
                                                    claw_redis::publish_job_event(&pool, &event.to_string()).await.ok();
                                                }
                                            }
                                        }
                                        // log_tx was moved into execute_chat_message and is
                                        // dropped when that function returns. Wait for the
                                        // forwarder task to drain any pending lines and exit
                                        // cleanly. (Inside the ensure_container Ok arm so
                                        // log_handle is in scope.)
                                        let _ = log_handle.await;
                                    }
                                    Err(e) => {
                                        tracing::error!(job_id = %job_id, error = %e, "Session container failed");
                                        claw_redis::fail_job(&pool, job_id, &format!("Container: {e}")).await.ok();
                                    }
                                }
                                continue;
                            }
                        }
                    }
                }
                } // end !is_task — tasks fall through to standard job path

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
                let skills: Vec<_> = skills.into_iter().filter(|s| s.enabled).collect();

                // 3b. Resolve tools (workspace defaults + job-specific, deduplicated)
                let mut all_tool_ids = Vec::new();
                if let Some(ref ws) = workspace {
                    all_tool_ids.extend(ws.tool_ids.iter().cloned());
                }
                all_tool_ids.extend(job.tool_ids.iter().cloned());
                let mut seen_tools = std::collections::HashSet::new();
                all_tool_ids.retain(|id| seen_tools.insert(id.clone()));
                let tools = claw_redis::resolve_tools(&pool, &all_tool_ids)
                    .await
                    .unwrap_or_default();
                let tools: Vec<_> = tools.into_iter().filter(|t| t.enabled).collect();

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
                let prepared_env = match environment::prepare_environment(&job, workspace.as_ref(), &skills, &tools, &pipeline_checkouts).await {
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

                // 4b. Verify local tools are available (check-only, no installation)
                if backend == executor::ExecutionBackend::Local && !tools.is_empty() {
                    if let Err(e) = environment::ensure_local_tools(&tools).await {
                        tracing::error!(job_id = %job_id, error = %e, "Local tool check failed");
                        claw_redis::fail_job(&pool, job_id, &format!("Tool check failed: {e}")).await.ok();
                        if let Some(ws_id) = job.workspace_id {
                            claw_redis::release_workspace_lock(&pool, ws_id, job_id).await.ok();
                        }
                        environment::teardown_environment(&prepared_env).await;
                        continue;
                    }
                }

                // 5. Build prompt (user prompt passes through unmodified)
                let built = prompt_builder::build_prompt(&job, &skills, &tools, workspace.as_ref());
                let system_prompt = built.system_prompt;
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

                // 5b. Emit workspace event: job started
                if let Some(ws_id) = job.workspace_id {
                    let prompt_preview: String = job.prompt.chars().take(100).collect();
                    let event = claw_models::WorkspaceEvent {
                        timestamp: chrono::Utc::now(),
                        event_type: claw_models::WorkspaceEventType::JobStarted,
                        related_id: Some(job_id.to_string()),
                        description: format!("Job started: {}", prompt_preview),
                    };
                    claw_redis::append_workspace_event(&pool, ws_id, &event).await.ok();
                }

                // 5c. Per-job Docker image check (cached 60s) when backend can change at runtime
                if backend == executor::ExecutionBackend::Docker {
                    let image = docker_config.as_ref().map(|c| c.image.as_str()).unwrap_or("claw-sandbox:latest");
                    if let Err(e) = docker::check_image_cached(image).await {
                        tracing::error!(job_id = %job_id, error = %e, "Docker sandbox image not available");
                        claw_redis::fail_job(&pool, job_id, &e).await.ok();
                        if let Some(ws_id) = job.workspace_id {
                            claw_redis::release_workspace_lock(&pool, ws_id, job_id).await.ok();
                        }
                        environment::teardown_environment(&prepared_env).await;
                        continue;
                    }
                }

                // 5b. Resolve credential env vars for tools
                let mut credential_env_vars = std::collections::HashMap::new();
                if let Some(ref ws) = workspace {
                    for tool in &tools {
                        if let Some(cred_id) = ws.credential_bindings.get(&tool.id) {
                            match claw_redis::get_credential_values(&pool, cred_id).await {
                                Ok(Some(values)) => {
                                    credential_env_vars.extend(values);
                                }
                                Ok(None) => {
                                    tracing::warn!(tool_id = %tool.id, credential_id = %cred_id, "Bound credential not found");
                                }
                                Err(e) => {
                                    tracing::warn!(tool_id = %tool.id, credential_id = %cred_id, error = %e, "Failed to decrypt credential");
                                }
                            }
                        }
                    }
                }

                // 5b. For chat jobs on local backend: deploy API skill + set env vars
                if is_chat_job && backend == executor::ExecutionBackend::Local {
                    if let Some(ws_id) = job.workspace_id {
                        session_container::deploy_api_skill(ws_id).await;
                    }
                    // Mint a session token for the user so Claude can call the API
                    if let Some(chat_tag) = job.tags.iter().find(|t| t.starts_with("chat:")) {
                        if let Ok(chat_id) = chat_tag.strip_prefix("chat:").unwrap_or("").parse::<uuid::Uuid>() {
                            if let Ok(Some(session)) = claw_redis::get_chat_session(&pool, chat_id).await {
                                if let Ok(token) = claw_redis::create_session(&pool, &session.user_id).await {
                                    let api_url = std::env::var("CLAW_CHAT_API_URL")
                                        .or_else(|_| std::env::var("CLAW_API_URL"))
                                        .unwrap_or_else(|_| "http://127.0.0.1:8080".to_string());
                                    credential_env_vars.insert("CLAW_API_URL".to_string(), api_url);
                                    credential_env_vars.insert("CLAW_SESSION".to_string(), token);
                                }
                            }
                        }
                    }
                }

                // 6. Execute in prepared workspace
                let result = executor::dispatch_execute(
                    &backend,
                    &job,
                    &prepared_env.working_dir,
                    docker_config.as_ref(),
                    workspace.as_ref(),
                    &tools,
                    &credential_env_vars,
                    anthropic_api_key.as_deref(),
                    Some(&system_prompt),
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

                // 8. Git snapshot: post-job commit (BEFORE teardown which deletes temp dirs)
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

                // 8b. Teardown environment (after git commit so temp dirs still exist)
                environment::teardown_environment(&prepared_env).await;

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

                        // Emit workspace event: job completed
                        if let Some(ws_id) = job.workspace_id {
                            let duration_str = format!("{:.1}s", r.duration_ms as f64 / 1000.0);
                            let result_preview: String = r.result_text.chars().take(200).collect();
                            let event = claw_models::WorkspaceEvent {
                                timestamp: chrono::Utc::now(),
                                event_type: claw_models::WorkspaceEventType::JobCompleted,
                                related_id: Some(job_id.to_string()),
                                description: format!("Job completed (${:.2}, {}): {}", r.cost_usd, duration_str, result_preview),
                            };
                            claw_redis::append_workspace_event(&pool, ws_id, &event).await.ok();
                        }

                        // Chat message: store assistant response if this is a chat job
                        if let Some(chat_tag) = job.tags.iter().find(|t| t.starts_with("chat:")) {
                            if let Ok(chat_id) = chat_tag.strip_prefix("chat:").unwrap_or("").parse::<uuid::Uuid>() {
                                let seq_tag = job.tags.iter().find(|t| t.starts_with("chat_seq:"));
                                let seq: u32 = seq_tag
                                    .and_then(|t| t.strip_prefix("chat_seq:"))
                                    .and_then(|s| s.parse().ok())
                                    .unwrap_or(0);
                                if seq > 0 {
                                    let assistant_msg = claw_models::ChatMessage {
                                        seq,
                                        role: "assistant".to_string(),
                                        content: r.result_text.clone(),
                                        summary: None,
                                        job_id: Some(job_id),
                                        cost_usd: Some(r.cost_usd),
                                        model: job.model.clone(),
                                        token_estimate: (r.result_text.len() / 4).max(1) as u32,
                                        files_written: Vec::new(),
                                        artifacts: Vec::new(),
                                        thinking: None,
                                        status: "complete".to_string(),
                                        timestamp: chrono::Utc::now(),
                                    };
                                    if let Err(e) = claw_redis::add_chat_message(&pool, chat_id, &assistant_msg).await {
                                        tracing::error!(chat_id = %chat_id, seq = seq, error = %e, "Failed to store chat response");
                                    } else {
                                        tracing::info!(chat_id = %chat_id, seq = seq, "Chat response stored");
                                        // Update session totals
                                        if let Ok(Some(mut session)) = claw_redis::get_chat_session(&pool, chat_id).await {
                                            session.total_messages += 1;
                                            session.total_cost_usd += r.cost_usd;
                                            session.last_activity = chrono::Utc::now();
                                            session.updated_at = chrono::Utc::now();
                                            claw_redis::update_chat_session(&pool, &session).await.ok();

                                            // Write message files to workspace for grep-based history
                                            let home = dirs::home_dir().unwrap_or_else(|| "/tmp".into());
                                            let checkout = home.join(".claw").join("checkouts").join(session.workspace_id.to_string());
                                            let messages_dir = checkout.join(".chat").join("messages");
                                            if let Err(e) = tokio::fs::create_dir_all(&messages_dir).await {
                                                tracing::warn!(error = %e, "Failed to create .chat/messages dir");
                                            } else {
                                                // Write assistant response file
                                                let assistant_path = messages_dir.join(format!("{:04}-assistant.md", seq));
                                                tokio::fs::write(&assistant_path, &r.result_text).await.ok();
                                                tracing::debug!(seq = seq, "Wrote chat message files to workspace");
                                            }
                                        }
                                    }
                                }
                            }
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
                        // Emit workspace event: job failed
                        if let Some(ws_id) = job.workspace_id {
                            let error_preview: String = e.chars().take(200).collect();
                            let event = claw_models::WorkspaceEvent {
                                timestamp: chrono::Utc::now(),
                                event_type: claw_models::WorkspaceEventType::JobFailed,
                                related_id: Some(job_id.to_string()),
                                description: format!("Job failed: {}", error_preview),
                            };
                            claw_redis::append_workspace_event(&pool, ws_id, &event).await.ok();
                        }

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
    run(&["-c", "user.name=Claw Machine", "-c", "user.email=claw@local", "commit", "-m", message]);
}
