use chrono::Utc;
use claw_models::*;
use deadpool_redis::Pool;
use redis::AsyncCommands;
use uuid::Uuid;

#[derive(Debug, thiserror::Error)]
pub enum RedisError {
    #[error("Redis error: {0}")]
    Redis(#[from] redis::RedisError),
    #[error("Pool error: {0}")]
    Pool(#[from] deadpool_redis::PoolError),
    #[error("Serialization error: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("Job not found: {0}")]
    NotFound(Uuid),
}

/// Submit a new job: store hash + push to pending queue.
pub async fn submit_job(pool: &Pool, req: &CreateJobRequest, source: JobSource) -> Result<Job, RedisError> {
    let mut conn = pool.get().await?;
    let now = Utc::now();
    let id = Uuid::new_v4();
    let priority = req.priority.unwrap_or(5);

    let job = Job {
        id,
        status: JobStatus::Pending,
        prompt: req.prompt.clone(),
        skill_ids: req.skill_ids.clone(),
        skill_tags: req.skill_tags.clone(),
        working_dir: req.working_dir.clone().unwrap_or_else(|| ".".into()),
        model: req.model.clone(),
        max_budget_usd: req.max_budget_usd,
        allowed_tools: req.allowed_tools.clone(),
        output_dest: req.output_dest.clone(),
        source,
        priority,
        tags: req.tags.clone(),
        created_at: now,
        started_at: None,
        completed_at: None,
        worker_id: None,
        error: None,
        cost_usd: None,
        duration_ms: None,
        retry_count: 0,
        timeout_secs: req.timeout_secs,
        workspace_id: req.workspace_id,
        template_id: req.template_id,
        cron_id: None,
        pipeline_run_id: None,
        pipeline_step: None,
        skill_snapshot: None,
        assembled_prompt: None,
    };

    let job_json = serde_json::to_string(&job)?;
    let job_key = format!("claw:job:{}", id);
    let queue_key = format!("claw:queue:pending:{}", priority.min(9));

    redis::pipe()
        .set(&job_key, &job_json)
        .rpush(&queue_key, id.to_string())
        .sadd("claw:jobs:all", id.to_string())
        .exec_async(&mut *conn)
        .await?;

    tracing::info!(job_id = %id, priority, "Job submitted");
    Ok(job)
}

/// Atomically claim the highest-priority pending job.
pub async fn claim_job(pool: &Pool, worker_id: &str) -> Result<Option<Job>, RedisError> {
    let mut conn = pool.get().await?;
    // Lua script: iterate priority queues 9→0, LPOP first non-empty
    let script = redis::Script::new(
        r#"
        for priority = 9, 0, -1 do
            local queue_key = 'claw:queue:pending:' .. priority
            local job_id = redis.call('LPOP', queue_key)
            if job_id then
                redis.call('SADD', 'claw:queue:running', job_id)
                return job_id
            end
        end
        return nil
        "#,
    );

    let result: Option<String> = script.invoke_async(&mut *conn).await?;

    let Some(job_id_str) = result else {
        return Ok(None);
    };

    let job_id: Uuid = job_id_str.parse().map_err(|_| {
        redis::RedisError::from((redis::ErrorKind::TypeError, "Invalid job UUID in queue"))
    })?;

    let job_key = format!("claw:job:{}", job_id);
    let job_json: String = conn.get(&job_key).await?;
    let mut job: Job = serde_json::from_str(&job_json)?;

    // Update status
    job.status = JobStatus::Running;
    job.started_at = Some(Utc::now());
    job.worker_id = Some(worker_id.to_string());

    let updated_json = serde_json::to_string(&job)?;
    let _: () = conn.set(&job_key, &updated_json).await?;

    tracing::info!(job_id = %job_id, worker_id, "Job claimed");
    Ok(Some(job))
}

/// Mark a job as completed with result data.
/// Uses a Lua script for atomicity: updates job, stores result, removes from running,
/// and increments stats counters in a single Redis operation.
pub async fn complete_job(
    pool: &Pool,
    job_id: Uuid,
    result_text: &str,
    cost_usd: f64,
    duration_ms: u64,
) -> Result<(), RedisError> {
    let mut conn = pool.get().await?;
    let now = Utc::now();
    let job_key = format!("claw:job:{}", job_id);

    let job_json: String = conn.get(&job_key).await?;
    let mut job: Job = serde_json::from_str(&job_json)?;

    job.status = JobStatus::Completed;
    job.completed_at = Some(now);
    job.cost_usd = Some(cost_usd);
    job.duration_ms = Some(duration_ms);

    let result_resp = JobResultResponse {
        job_id,
        result: result_text.to_string(),
        cost_usd,
        duration_ms,
    };

    let updated_json = serde_json::to_string(&job)?;
    let result_json = serde_json::to_string(&result_resp)?;
    let result_key = format!("claw:job:{}:result", job_id);

    // Atomic Lua script: update job + store result + remove from running + update stats
    let script = redis::Script::new(
        r#"
        redis.call('SET', KEYS[1], ARGV[1])
        redis.call('SET', KEYS[2], ARGV[2])
        redis.call('SREM', 'claw:queue:running', ARGV[3])
        redis.call('INCR', 'claw:stats:total_completed')
        redis.call('INCRBYFLOAT', 'claw:stats:total_cost_usd', ARGV[4])
        return 1
        "#,
    );

    let _: i32 = script
        .key(&job_key)
        .key(&result_key)
        .arg(&updated_json)
        .arg(&result_json)
        .arg(job_id.to_string())
        .arg(cost_usd)
        .invoke_async(&mut *conn)
        .await?;

    tracing::info!(job_id = %job_id, cost_usd, duration_ms, "Job completed");
    Ok(())
}

/// Mark a job as failed with an error message.
/// If the job has retries remaining (max 3), it is re-queued instead.
/// Returns true if the job was re-queued for retry, false if terminally failed.
pub async fn fail_job(pool: &Pool, job_id: Uuid, error: &str) -> Result<bool, RedisError> {
    let mut conn = pool.get().await?;
    let job_key = format!("claw:job:{}", job_id);

    let job_json: String = conn.get(&job_key).await?;
    let mut job: Job = serde_json::from_str(&job_json)?;

    let max_retries: u32 = 3;
    // Only auto-retry execution failures, not cancellations or pipeline jobs
    let should_retry = job.retry_count < max_retries
        && job.pipeline_run_id.is_none()
        && !error.contains("cancelled");

    if should_retry {
        job.status = JobStatus::Pending;
        job.worker_id = None;
        job.started_at = None;
        job.retry_count += 1;
        job.error = Some(format!("Retry {}/{}: {}", job.retry_count, max_retries, error));

        let updated_json = serde_json::to_string(&job)?;
        let queue_key = format!("claw:queue:pending:{}", job.priority.min(9));

        redis::pipe()
            .set(&job_key, &updated_json)
            .srem("claw:queue:running", job_id.to_string())
            .rpush(&queue_key, job_id.to_string())
            .exec_async(&mut *conn)
            .await?;

        tracing::warn!(job_id = %job_id, retry_count = job.retry_count, error, "Job failed, re-queued for retry");
        Ok(true)
    } else {
        job.status = JobStatus::Failed;
        job.error = Some(error.to_string());
        job.completed_at = Some(Utc::now());

        let updated_json = serde_json::to_string(&job)?;

        // Atomic Lua script: update job + remove from running + increment failed counter
        let script = redis::Script::new(
            r#"
            redis.call('SET', KEYS[1], ARGV[1])
            redis.call('SREM', 'claw:queue:running', ARGV[2])
            redis.call('INCR', 'claw:stats:total_failed')
            return 1
            "#,
        );

        let _: i32 = script
            .key(&job_key)
            .arg(&updated_json)
            .arg(job_id.to_string())
            .invoke_async(&mut *conn)
            .await?;

        tracing::warn!(job_id = %job_id, error, "Job failed (terminal)");
        Ok(false)
    }
}

/// Mark a job as cancelled.
pub async fn cancel_job(pool: &Pool, job_id: Uuid) -> Result<(), RedisError> {
    let mut conn = pool.get().await?;
    let job_key = format!("claw:job:{}", job_id);

    let job_json: Option<String> = conn.get(&job_key).await?;
    let Some(json) = job_json else {
        return Err(RedisError::NotFound(job_id));
    };
    let mut job: Job = serde_json::from_str(&json)?;

    match job.status {
        JobStatus::Pending => {
            // Remove from pending queue
            for p in 0..=9 {
                let queue_key = format!("claw:queue:pending:{}", p);
                let _: () = redis::cmd("LREM")
                    .arg(&queue_key)
                    .arg(0)
                    .arg(job_id.to_string())
                    .query_async(&mut *conn)
                    .await?;
            }
        }
        JobStatus::Running => {
            let _: () = redis::cmd("SREM")
                .arg("claw:queue:running")
                .arg(job_id.to_string())
                .query_async(&mut *conn)
                .await?;
        }
        _ => {}
    }

    job.status = JobStatus::Cancelled;
    job.completed_at = Some(Utc::now());
    let updated_json = serde_json::to_string(&job)?;
    let _: () = conn.set(&job_key, &updated_json).await?;

    tracing::info!(job_id = %job_id, "Job cancelled");
    Ok(())
}

/// Delete a job and all associated data.
pub async fn delete_job(pool: &Pool, job_id: Uuid) -> Result<(), RedisError> {
    let mut conn = pool.get().await?;
    redis::pipe()
        .del(format!("claw:job:{}", job_id))
        .del(format!("claw:job:{}:result", job_id))
        .del(format!("claw:job:{}:log", job_id))
        .del(format!("claw:job:{}:cancel", job_id))
        .srem("claw:jobs:all", job_id.to_string())
        .exec_async(&mut *conn)
        .await?;
    tracing::info!(job_id = %job_id, "Job deleted");
    Ok(())
}

/// Update skill_snapshot and assembled_prompt fields on a job.
pub async fn update_job_fields(
    pool: &Pool,
    job_id: Uuid,
    skill_snapshot: &Option<serde_json::Value>,
    assembled_prompt: &Option<String>,
) -> Result<(), RedisError> {
    let mut conn = pool.get().await?;
    let job_key = format!("claw:job:{}", job_id);
    let job_json: String = conn.get(&job_key).await?;
    let mut job: Job = serde_json::from_str(&job_json)?;
    job.skill_snapshot = skill_snapshot.clone();
    job.assembled_prompt = assembled_prompt.clone();
    let updated_json = serde_json::to_string(&job)?;
    let _: () = conn.set(&job_key, &updated_json).await?;
    Ok(())
}

/// Get a single job by ID.
pub async fn get_job(pool: &Pool, job_id: Uuid) -> Result<Job, RedisError> {
    let mut conn = pool.get().await?;
    let job_key = format!("claw:job:{}", job_id);
    let job_json: Option<String> = conn.get(&job_key).await?;
    let json = job_json.ok_or(RedisError::NotFound(job_id))?;
    Ok(serde_json::from_str(&json)?)
}

/// Get a job's result.
pub async fn get_result(pool: &Pool, job_id: Uuid) -> Result<JobResultResponse, RedisError> {
    let mut conn = pool.get().await?;
    let key = format!("claw:job:{}:result", job_id);
    let json: Option<String> = conn.get(&key).await?;
    let json = json.ok_or(RedisError::NotFound(job_id))?;
    Ok(serde_json::from_str(&json)?)
}

/// List jobs from the global index set, filter by status and/or workspace.
pub async fn list_jobs(pool: &Pool, status_filter: Option<JobStatus>, limit: usize, workspace_filter: Option<Uuid>) -> Result<Vec<Job>, RedisError> {
    let mut conn = pool.get().await?;

    let job_ids: Vec<String> = redis::cmd("SMEMBERS")
        .arg("claw:jobs:all")
        .query_async(&mut *conn)
        .await
        .unwrap_or_default();

    let mut jobs = Vec::new();
    for id in &job_ids {
        let key = format!("claw:job:{}", id);
        if let Ok(json) = conn.get::<_, String>(&key).await {
            if let Ok(job) = serde_json::from_str::<Job>(&json) {
                if let Some(ref filter) = status_filter {
                    if &job.status != filter {
                        continue;
                    }
                }
                if let Some(ws_id) = workspace_filter {
                    if job.workspace_id != Some(ws_id) {
                        continue;
                    }
                }
                jobs.push(job);
            }
        }
    }

    // Sort by created_at descending
    jobs.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    jobs.truncate(limit);
    Ok(jobs)
}

/// Get queue status counts.
pub async fn get_queue_status(pool: &Pool) -> Result<QueueStatus, RedisError> {
    let mut conn = pool.get().await?;

    let mut pending: u64 = 0;
    for p in 0..=9 {
        let key = format!("claw:queue:pending:{}", p);
        let len: u64 = redis::cmd("LLEN")
            .arg(&key)
            .query_async(&mut *conn)
            .await
            .unwrap_or(0);
        pending += len;
    }

    let running: u64 = redis::cmd("SCARD")
        .arg("claw:queue:running")
        .query_async(&mut *conn)
        .await
        .unwrap_or(0);

    let completed: u64 = conn
        .get::<_, Option<u64>>("claw:stats:total_completed")
        .await
        .unwrap_or(None)
        .unwrap_or(0);

    let failed: u64 = conn
        .get::<_, Option<u64>>("claw:stats:total_failed")
        .await
        .unwrap_or(None)
        .unwrap_or(0);

    Ok(QueueStatus {
        pending,
        running,
        completed,
        failed,
    })
}
