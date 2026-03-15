# Worker Engine — Job Execution Architecture

## 1. Overview

The worker engine is the core of ClaudeCodeClaw. It claims jobs from Redis, builds rich prompts by injecting skills, spawns `claude -p` as a subprocess, streams output in real-time, and routes results to configured destinations.

## 2. Worker Process Lifecycle

```
┌─────────────────────────────────────────────────────────┐
│                    claw-worker process                    │
│                                                          │
│  main()                                                  │
│    │                                                     │
│    ├── Load config (env vars → config.toml → defaults)   │
│    ├── Connect to Redis (deadpool pool)                  │
│    ├── Generate worker ID: "{hostname}-{pid}"            │
│    ├── Seed skills from filesystem (if configured)       │
│    │                                                     │
│    ├── Spawn async tasks:                                │
│    │   ├── Worker Task 0 ─── claim → execute loop        │
│    │   ├── Worker Task 1 ─── claim → execute loop        │
│    │   ├── Heartbeat Task ── refresh TTL keys / 10s      │
│    │   └── Reaper Task ───── check dead workers / 15s    │
│    │                                                     │
│    ├── Register signal handlers (SIGTERM, SIGINT)        │
│    │                                                     │
│    └── tokio::select! {                                  │
│          _ = shutdown_signal => graceful_shutdown()       │
│          _ = all_tasks_complete => unreachable            │
│        }                                                 │
│                                                          │
│  graceful_shutdown():                                    │
│    1. Set shutdown flag (AtomicBool)                     │
│    2. Worker tasks stop claiming new jobs                │
│    3. Wait for in-flight jobs to complete (with timeout) │
│    4. Delete heartbeat keys                              │
│    5. Exit 0                                             │
└─────────────────────────────────────────────────────────┘
```

## 3. Worker Task Loop

Each worker task (there are N per process, configurable) runs this loop:

```
┌────────────────────────────────────────────┐
│            Worker Task Loop                 │
│                                            │
│  loop {                                    │
│    if shutdown_flag.load() { break; }      │
│                                            │
│    // 1. Attempt to claim a job            │
│    job_id = redis.eval(claim_lua_script)   │
│    if job_id is None {                     │
│      // No jobs available                  │
│      // BLPOP with 5s timeout on           │
│      // a notification list, or just       │
│      // sleep 2s and retry                 │
│      sleep(2s);                            │
│      continue;                             │
│    }                                       │
│                                            │
│    // 2. Load full job data                │
│    job = redis.hgetall(job:{job_id})       │
│                                            │
│    // 3. Prepare execution environment     │
│    env = prepare_environment(&job)         │
│                                            │
│    // 4. Build the prompt                  │
│    skills = resolve_skills(&job)           │
│    prompt = build_prompt(&job, &skills)    │
│                                            │
│    // 4b. Snapshot skills + prompt for     │
│    //     reproducibility                  │
│    store_snapshot(&job, &skills, &prompt)  │
│                                            │
│    // 5. Execute                           │
│    result = execute(&job, &prompt, &env)   │
│                                            │
│    // 6. Handle result                     │
│    match result {                          │
│      Ok(r) => complete_job(&job, &r),      │
│      Err(e) => fail_job(&job, &e),         │
│    }                                       │
│  }                                         │
└────────────────────────────────────────────┘
```

## 4. Environment Preparation

Before spawning `claude -p`, the worker prepares the execution environment:

### 4.1 Working Directory Setup

```
1. Resolve working_dir:
   - If job specifies working_dir → use it
   - If not → create a temp dir under CLAW_WORKSPACES_DIR

2. Ensure directory exists (create if needed)

3. If job has CLAUDE.md config skills:
   - Read existing {working_dir}/CLAUDE.md (if any)
   - Back up original to {working_dir}/.claw/CLAUDE.md.backup.{job_id}
   - Append skill content separated by section headers
   - Write merged CLAUDE.md
   - Write marker file {working_dir}/.claw/injected-{job_id} for crash recovery

4. If job has script skills:
   - Create {working_dir}/.claw/scripts/{job_id}/ directory (job-specific)
   - Write each script skill to a file
   - chmod +x each script
   - Add to PATH or mention in prompt

5. Crash recovery (on worker startup):
   - Scan all known working directories for stale .claw/injected-* markers
   - For each marker found, restore CLAUDE.md from backup and remove artifacts
   - This ensures working directories are not polluted after unclean shutdowns
```

### 4.2 Environment Variables

The subprocess inherits a controlled set of environment variables:

```rust
fn build_env(job: &Job, config: &WorkerConfig) -> HashMap<String, String> {
    let mut env = HashMap::new();

    // Claude Code uses OAuth — the worker inherits the host user's session.
    // HOME must point to a directory containing ~/.claude/ with OAuth tokens.
    // No ANTHROPIC_API_KEY needed when using OAuth auth.
    env.insert("HOME", config.home_dir.clone());
    env.insert("PATH", config.path.clone());

    // Prevent claude from trying to use a TTY
    env.insert("TERM", "dumb".into());
    env.insert("NO_COLOR", "1".into());

    // Pass job metadata as env vars (accessible to scripts)
    env.insert("CLAW_JOB_ID", job.id.to_string());
    env.insert("CLAW_JOB_SOURCE", job.source.to_string());

    env
}
```

## 5. Prompt Builder

The prompt builder assembles the final prompt from the job definition and resolved skills.

### 5.1 Prompt Assembly Order

```
┌─────────────────────────────────────────┐
│           Final Prompt Structure          │
│                                          │
│  1. Skill template injections            │
│     <skill name="code-review">           │
│       When reviewing code...             │
│     </skill>                             │
│     <skill name="rust-conventions">      │
│       Use thiserror for errors...        │
│     </skill>                             │
│                                          │
│  2. Available scripts notice             │
│     You have access to scripts in        │
│     .claw/scripts/: run-tests.sh,        │
│     lint-check.sh                        │
│                                          │
│  3. Context metadata                     │
│     [Job ID: abc-123]                    │
│     [Source: cron]                        │
│     [Working dir: /repos/project]        │
│                                          │
│  4. The actual user prompt               │
│     Review all open PRs and summarize    │
│     findings with severity ratings.      │
│                                          │
└─────────────────────────────────────────┘
```

### 5.2 Skill Resolution

```rust
async fn resolve_skills(
    job: &Job,
    redis: &RedisPool,
) -> Vec<Skill> {
    let mut skills = Vec::new();
    let mut seen_ids = HashSet::new();

    // 1. Explicitly referenced skills by ID
    for skill_id in &job.skill_ids {
        if let Some(skill) = redis::get_skill(redis, skill_id).await {
            seen_ids.insert(skill.id.clone());
            skills.push(skill);
        } else {
            tracing::warn!(skill_id, "Referenced skill not found");
        }
    }

    // 2. Tag-matched skills (auto-discovery)
    if !job.skill_tags.is_empty() {
        let all_skills = redis::list_skills(redis).await;
        for skill in all_skills {
            if seen_ids.contains(&skill.id) {
                continue; // Already included by ID
            }
            // Include if any job tag matches any skill tag
            let matches = skill.tags.iter().any(|t| job.skill_tags.contains(t));
            if matches {
                seen_ids.insert(skill.id.clone());
                skills.push(skill);
            }
        }
    }

    skills
}
```

### 5.3 Prompt Construction

```rust
pub fn build_prompt(job: &Job, skills: &[Skill]) -> String {
    let mut sections: Vec<String> = Vec::new();

    // Inject template skills
    let templates: Vec<_> = skills.iter()
        .filter(|s| s.skill_type == SkillType::Template)
        .collect();

    if !templates.is_empty() {
        for skill in &templates {
            sections.push(format!(
                "<skill name=\"{}\">\n{}\n</skill>",
                skill.id, skill.content
            ));
        }
    }

    // Notify about available scripts
    let scripts: Vec<_> = skills.iter()
        .filter(|s| s.skill_type == SkillType::Script)
        .collect();

    if !scripts.is_empty() {
        let script_names: Vec<_> = scripts.iter()
            .map(|s| format!("{}.sh", s.id))
            .collect();
        sections.push(format!(
            "You have access to the following executable scripts in .claw/scripts/: {}. \
             Run them with bash if needed for your task.",
            script_names.join(", ")
        ));
    }

    // Add context metadata
    sections.push(format!(
        "[Job ID: {}] [Source: {}] [Working dir: {}]",
        job.id, job.source, job.working_dir.display()
    ));

    // The actual prompt
    sections.push(job.prompt.clone());

    let assembled = sections.join("\n\n");

    // Prompt size validation: warn if assembled prompt is very large
    if assembled.len() > 100_000 {
        tracing::warn!(
            job_id = %job.id,
            prompt_len = assembled.len(),
            "Assembled prompt exceeds 100K characters — may hit context window limits"
        );
    }

    assembled
}
```

## 6. Process Executor

### 6.1 Subprocess Spawning

```rust
use tokio::process::Command;
use tokio::io::{AsyncBufReadExt, BufReader};

pub struct ExecutionResult {
    pub exit_code: i32,
    pub result_json: Option<serde_json::Value>,
    pub cost_usd: f64,
    pub duration_ms: u64,
    pub session_id: Option<String>,
}

pub struct ExecutionError {
    pub message: String,
    pub exit_code: Option<i32>,
    pub stderr: String,
}

pub async fn execute_job(
    prompt: &str,
    working_dir: &Path,
    env: &HashMap<String, String>,
    model: Option<&str>,
    max_budget: Option<f64>,
    allowed_tools: Option<&[String]>,
    timeout: Duration,
    log_sender: mpsc::Sender<String>,
    cancel_token: CancellationToken,
) -> Result<ExecutionResult, ExecutionError> {

    // Build command
    let mut cmd = Command::new("claude");
    cmd.arg("-p").arg(prompt);
    cmd.arg("--output-format").arg("stream-json");
    cmd.arg("--verbose");
    cmd.arg("--dangerously-skip-permissions");
    cmd.current_dir(working_dir);
    cmd.envs(env);

    if let Some(m) = model {
        cmd.arg("--model").arg(m);
    }
    if let Some(budget) = max_budget {
        cmd.arg("--max-turns").arg("200"); // safety cap
    }
    if let Some(tools) = allowed_tools {
        for tool in tools {
            cmd.arg("--allowedTools").arg(tool);
        }
    }

    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    cmd.kill_on_drop(true); // safety: kill child if task is dropped

    let start = Instant::now();
    let mut child = cmd.spawn().map_err(|e| ExecutionError {
        message: format!("Failed to spawn claude: {}", e),
        exit_code: None,
        stderr: String::new(),
    })?;

    // Stream stdout
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();

    let stdout_reader = BufReader::new(stdout);
    let stderr_reader = BufReader::new(stderr);

    let mut stdout_lines = stdout_reader.lines();
    let mut stderr_lines = stderr_reader.lines();
    let mut stderr_output = String::new();
    let mut final_result: Option<serde_json::Value> = None;

    // Process output with timeout and cancellation
    let result = tokio::select! {
        // Normal execution
        r = async {
            loop {
                tokio::select! {
                    line = stdout_lines.next_line() => {
                        match line {
                            Ok(Some(line)) => {
                                // Try to parse as JSON
                                if let Ok(val) = serde_json::from_str::<serde_json::Value>(&line) {
                                    if val.get("type").and_then(|t| t.as_str()) == Some("result") {
                                        final_result = Some(val);
                                    }
                                }
                                // Forward to log channel (best effort)
                                let _ = log_sender.send(line).await;
                            }
                            Ok(None) => break, // EOF
                            Err(e) => {
                                tracing::warn!("Error reading stdout: {}", e);
                                break;
                            }
                        }
                    }
                    line = stderr_lines.next_line() => {
                        if let Ok(Some(line)) = line {
                            stderr_output.push_str(&line);
                            stderr_output.push('\n');
                        }
                    }
                }
            }
            child.wait().await
        } => r,

        // Timeout
        _ = tokio::time::sleep(timeout) => {
            child.kill().await.ok();
            return Err(ExecutionError {
                message: format!("Job timed out after {:?}", timeout),
                exit_code: None,
                stderr: stderr_output,
            });
        }

        // Cancellation (job was cancelled via API/CLI)
        _ = cancel_token.cancelled() => {
            child.kill().await.ok();
            return Err(ExecutionError {
                message: "Job was cancelled".to_string(),
                exit_code: None,
                stderr: stderr_output,
            });
        }
    };

    let exit_status = result.map_err(|e| ExecutionError {
        message: format!("Failed to wait for claude: {}", e),
        exit_code: None,
        stderr: stderr_output.clone(),
    })?;

    let duration_ms = start.elapsed().as_millis() as u64;
    let exit_code = exit_status.code().unwrap_or(-1);

    if !exit_status.success() {
        return Err(ExecutionError {
            message: format!("claude exited with code {}", exit_code),
            exit_code: Some(exit_code),
            stderr: stderr_output,
        });
    }

    // Extract cost from result JSON
    let cost_usd = final_result.as_ref()
        .and_then(|r| r.get("cost_usd"))
        .and_then(|c| c.as_f64())
        .unwrap_or(0.0);

    let session_id = final_result.as_ref()
        .and_then(|r| r.get("session_id"))
        .and_then(|s| s.as_str())
        .map(String::from);

    Ok(ExecutionResult {
        exit_code,
        result_json: final_result,
        cost_usd,
        duration_ms,
        session_id,
    })
}
```

### 6.2 Log Streaming Pipeline

```
claude -p stdout
    │
    │ (line by line, tokio AsyncBufReadExt)
    ▼
mpsc::Sender<String>
    │
    │ (log forwarder task)
    ▼
┌───┴────────────────────────────┐
│                                │
│  1. RPUSH claw:job:{id}:log   │  (persistent storage)
│                                │
│  2. PUBLISH claw:events:       │  (real-time to WebSocket)
│     logs:{job_id}              │
│                                │
└────────────────────────────────┘
```

The log forwarder task runs concurrently with the executor. It receives lines from the mpsc channel and writes them to both Redis storage (for later retrieval) and Redis pub/sub (for live streaming to the UI).

```rust
async fn forward_logs(
    job_id: &Uuid,
    mut receiver: mpsc::Receiver<String>,
    redis: &RedisPool,
) {
    let log_key = format!("claw:job:{}:log", job_id);
    let pubsub_channel = format!("claw:events:logs:{}", job_id);

    while let Some(line) = receiver.recv().await {
        let mut conn = redis.get().await.unwrap();

        // Store persistently
        let _: () = redis::cmd("RPUSH")
            .arg(&log_key)
            .arg(&line)
            .query_async(&mut conn)
            .await
            .unwrap_or_default();

        // Publish for real-time streaming
        let event = serde_json::json!({
            "type": "job_log",
            "job_id": job_id.to_string(),
            "line": line,
            "timestamp": Utc::now().to_rfc3339(),
        });
        let _: () = redis::cmd("PUBLISH")
            .arg(&pubsub_channel)
            .arg(event.to_string())
            .query_async(&mut conn)
            .await
            .unwrap_or_default();
    }
}
```

## 7. Output Routing

After successful execution, results are routed based on the job's `output_dest`:

```rust
pub async fn route_output(
    job: &Job,
    result: &ExecutionResult,
    redis: &RedisPool,
    http_client: &reqwest::Client,
) -> Result<(), OutputError> {

    // Extract the text result from the stream-json output
    let result_text = result.result_json.as_ref()
        .and_then(|r| r.get("result"))
        .and_then(|r| r.as_str())
        .unwrap_or("");

    let result_payload = serde_json::json!({
        "job_id": job.id.to_string(),
        "status": "completed",
        "result": result_text,
        "cost_usd": result.cost_usd,
        "duration_ms": result.duration_ms,
        "model": job.model,
        "completed_at": Utc::now().to_rfc3339(),
    });

    // Always store in Redis (regardless of output_dest)
    let result_key = format!("claw:job:{}:result", job.id);
    redis::set(redis, &result_key, &result_payload.to_string()).await?;

    // Route to configured destination
    match &job.output_dest {
        OutputDest::Redis => {
            // Already stored above
        }
        OutputDest::File { path } => {
            let file_path = PathBuf::from(path)
                .join(format!("{}.json", job.id));
            tokio::fs::create_dir_all(path).await?;
            tokio::fs::write(&file_path, serde_json::to_string_pretty(&result_payload)?).await?;
            tracing::info!(path = %file_path.display(), "Result written to file");
        }
        OutputDest::Webhook { url } => {
            let response = http_client
                .post(url)
                .json(&result_payload)
                .timeout(Duration::from_secs(30))
                .send()
                .await;

            match response {
                Ok(resp) if resp.status().is_success() => {
                    tracing::info!(url, status = %resp.status(), "Webhook delivered");
                }
                Ok(resp) => {
                    tracing::error!(url, status = %resp.status(), "Webhook returned error");
                    // Don't fail the job — result is still in Redis
                }
                Err(e) => {
                    tracing::error!(url, error = %e, "Webhook delivery failed");
                    // Don't fail the job — result is still in Redis
                }
            }
        }
    }

    Ok(())
}
```

## 8. Heartbeat System

### 8.1 Heartbeat Publisher

```rust
async fn heartbeat_loop(
    worker_id: &str,
    task_ids: &[String],
    redis: &RedisPool,
    shutdown: &AtomicBool,
) {
    let mut interval = tokio::time::interval(Duration::from_secs(10));

    loop {
        interval.tick().await;

        if shutdown.load(Ordering::Relaxed) {
            // Clean up heartbeat keys on shutdown
            for task_id in task_ids {
                let key = format!("claw:worker:{}:heartbeat", task_id);
                redis::del(redis, &key).await.ok();
            }
            break;
        }

        let now = Utc::now().timestamp();
        for task_id in task_ids {
            let key = format!("claw:worker:{}:heartbeat", task_id);
            // SET with 30s TTL — if we stop refreshing, key auto-expires
            redis::set_ex(redis, &key, &now.to_string(), 30).await.ok();
        }
    }
}
```

### 8.2 Dead Worker Reaper

The reaper is protected by two mechanisms to prevent race conditions:

1. **Leader lease**: Only one worker runs the reaper at a time, enforced via `SETNX claw:reaper:leader` with a 20-second TTL. Other workers skip the reaper scan if the lease is held.
2. **Atomic Lua re-queue**: Each re-queue operation uses `reaper_requeue.lua` (see data model doc) which atomically checks heartbeat, verifies the job is still in the running set, and either re-queues or fails it. This prevents duplicate re-queuing even if the leader lease hand-off races.

```rust
async fn reaper_loop(worker_id: &str, redis: &RedisPool, shutdown: &AtomicBool) {
    let mut interval = tokio::time::interval(Duration::from_secs(15));

    loop {
        interval.tick().await;
        if shutdown.load(Ordering::Relaxed) { break; }

        // Attempt to acquire leader lease (20s TTL)
        let acquired: bool = redis::set_nx_ex(
            redis, "claw:reaper:leader", worker_id, 20
        ).await.unwrap_or(false);

        if !acquired {
            continue; // Another worker is the reaper leader
        }

        // Get all running job IDs
        let running: Vec<String> = redis::smembers(redis, "claw:queue:running").await
            .unwrap_or_default();

        for job_id in running {
            let job_key = format!("claw:job:{}", job_id);
            let worker_id: Option<String> = redis::hget(redis, &job_key, "worker_id").await.ok();
            let Some(wid) = worker_id else { continue };

            // Use atomic Lua script to check heartbeat + re-queue
            let result: i32 = redis::eval_reaper_requeue(
                redis, &job_id, &wid, 3, &Utc::now().to_rfc3339()
            ).await.unwrap_or(0);

            match result {
                1 => tracing::warn!(job_id, worker_id = %wid, "Dead worker: job re-queued"),
                -1 => tracing::error!(job_id, worker_id = %wid, "Dead worker: max retries exceeded"),
                _ => {} // Worker alive or already handled
            }
        }
    }
}
```

## 9. Job Cancellation

Cancellation is cooperative. When a cancel request arrives:

1. **For pending jobs**: Simple — `LREM` from the pending queue, `HSET status=cancelled`
2. **For running jobs**: The API sets a cancellation flag in Redis (`HSET claw:job:{id}:cancel 1`). The worker task periodically checks this flag (or subscribes via pub/sub) and triggers the `CancellationToken`, which kills the subprocess.

```rust
// In the worker task loop, spawned alongside the executor
async fn watch_for_cancellation(
    job_id: &Uuid,
    redis: &RedisPool,
    cancel_token: CancellationToken,
) {
    let cancel_key = format!("claw:job:{}:cancel", job_id);
    let mut interval = tokio::time::interval(Duration::from_secs(2));

    loop {
        interval.tick().await;
        let cancelled: bool = redis::exists(redis, &cancel_key)
            .await
            .unwrap_or(false);

        if cancelled {
            cancel_token.cancel();
            break;
        }
    }
}
```

## 10. Concurrency Safety

### 10.1 Job Claiming

The Lua script ensures atomic claiming — no two workers can claim the same job. This is the only synchronization point between workers.

### 10.2 Working Directory Isolation

Each job should operate in its own working directory to prevent conflicts between parallel jobs. Options:

- **Separate repos**: Each job points to a different repository/directory (natural for independent tasks)
- **Git worktrees**: For jobs operating on the same repo, create a git worktree per job
- **Temp directories**: For jobs that don't need a specific directory, use a fresh temp dir

The worker does NOT enforce isolation — it's the job submitter's responsibility to choose appropriate working directories. However, the default behavior creates a unique temp directory if `working_dir` is not specified.

### 10.3 Skill Write Safety

CLAUDE.md and script files are written to the working directory before execution and cleaned up after. If two jobs share the same `working_dir` (not recommended for parallel execution), the skill injection could conflict. The worker uses a job-specific subdirectory for scripts (`.claw/scripts/{job_id}/`) to mitigate this.

## 11. Resource Limits

| Resource | Limit | Enforcement |
|----------|-------|-------------|
| Execution time | Configurable per-job, default 30 min | `tokio::time::timeout` + process kill |
| API cost | Per-job `max_budget_usd` | Passed to `claude --max-turns` (approximate) |
| Concurrent jobs | Workers × concurrency | Worker task pool size |
| Log storage | Cleaned up with job | Redis memory |
| Stdout buffer | Unbounded (streamed) | Lines processed as they arrive |

## 12. Observability

### 12.1 Structured Logging

All worker components use `tracing` with structured fields:

```rust
tracing::info!(
    job_id = %job.id,
    worker_id = %task_id,
    prompt_len = job.prompt.len(),
    skills = ?job.skill_ids,
    "Job claimed, starting execution"
);
```

Log output format (via `tracing-subscriber`):
- Development: human-readable with colors
- Production (Docker): JSON lines for log aggregation

### 12.2 Metrics (Future)

Planned Prometheus metrics:
- `claw_jobs_total{status}` — counter by final status
- `claw_job_duration_seconds` — histogram of execution times
- `claw_job_cost_usd` — histogram of per-job costs
- `claw_worker_busy` — gauge of busy worker tasks
- `claw_queue_depth{priority}` — gauge of pending jobs per priority level
