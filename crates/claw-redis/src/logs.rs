use deadpool_redis::Pool;
use redis::AsyncCommands;
use uuid::Uuid;

use crate::RedisError;

/// Append a log line to a job's log list and publish for live streaming.
pub async fn append_log(pool: &Pool, job_id: Uuid, line: &str) -> Result<(), RedisError> {
    let mut conn = pool.get().await?;
    let log_key = format!("claw:job:{}:log", job_id);
    let pubsub_channel = format!("claw:events:logs:{}", job_id);

    redis::pipe()
        .rpush(&log_key, line)
        .publish(&pubsub_channel, line)
        .exec_async(&mut *conn)
        .await?;

    Ok(())
}

/// Get stored log lines for a job.
pub async fn get_logs(pool: &Pool, job_id: Uuid, offset: usize, limit: usize) -> Result<Vec<String>, RedisError> {
    let mut conn = pool.get().await?;
    let log_key = format!("claw:job:{}:log", job_id);
    let end = if limit == 0 { -1 } else { (offset + limit - 1) as isize };

    let lines: Vec<String> = redis::cmd("LRANGE")
        .arg(&log_key)
        .arg(offset as isize)
        .arg(end)
        .query_async(&mut *conn)
        .await?;

    Ok(lines)
}

/// Publish a job state event to the PubSub channel.
pub async fn publish_job_event(pool: &Pool, event_json: &str) -> Result<(), RedisError> {
    let mut conn = pool.get().await?;
    let _: () = redis::cmd("PUBLISH")
        .arg("claw:events:jobs")
        .arg(event_json)
        .query_async(&mut *conn)
        .await?;
    Ok(())
}

/// Cancel a job: set a cancel flag in Redis.
pub async fn set_cancel_flag(pool: &Pool, job_id: Uuid) -> Result<(), RedisError> {
    let mut conn = pool.get().await?;
    let _: () = conn.set(format!("claw:job:{}:cancel", job_id), "1").await?;
    Ok(())
}

/// Check if a job has been cancelled.
pub async fn is_cancelled(pool: &Pool, job_id: Uuid) -> Result<bool, RedisError> {
    let mut conn = pool.get().await?;
    let exists: bool = conn.exists(format!("claw:job:{}:cancel", job_id)).await?;
    Ok(exists)
}

/// Clean up cancel flag after job finishes.
pub async fn clear_cancel_flag(pool: &Pool, job_id: Uuid) -> Result<(), RedisError> {
    let mut conn = pool.get().await?;
    let _: () = conn.del(format!("claw:job:{}:cancel", job_id)).await?;
    Ok(())
}

/// Set worker heartbeat with TTL.
pub async fn set_heartbeat(pool: &Pool, worker_id: &str, ttl_secs: u64) -> Result<(), RedisError> {
    let mut conn = pool.get().await?;
    let key = format!("claw:worker:{}:heartbeat", worker_id);
    let now = chrono::Utc::now().timestamp().to_string();
    let _: () = redis::cmd("SET")
        .arg(&key)
        .arg(&now)
        .arg("EX")
        .arg(ttl_secs)
        .query_async(&mut *conn)
        .await?;
    Ok(())
}

/// Check if a worker's heartbeat exists.
pub async fn heartbeat_exists(pool: &Pool, worker_id: &str) -> Result<bool, RedisError> {
    let mut conn = pool.get().await?;
    let key = format!("claw:worker:{}:heartbeat", worker_id);
    let exists: bool = conn.exists(&key).await?;
    Ok(exists)
}

/// Delete heartbeat key (on shutdown).
pub async fn delete_heartbeat(pool: &Pool, worker_id: &str) -> Result<(), RedisError> {
    let mut conn = pool.get().await?;
    let key = format!("claw:worker:{}:heartbeat", worker_id);
    let _: () = conn.del(&key).await?;
    Ok(())
}

/// Count active workers by counting heartbeat keys.
pub async fn count_active_workers(pool: &Pool) -> Result<u64, RedisError> {
    let mut conn = pool.get().await?;
    let keys: Vec<String> = redis::cmd("KEYS")
        .arg("claw:worker:*:heartbeat")
        .query_async(&mut *conn)
        .await
        .unwrap_or_default();
    Ok(keys.len() as u64)
}

/// Find running jobs whose workers have dead heartbeats and re-queue them.
/// Uses a leader lease to prevent multiple reapers from running simultaneously.
/// Returns the number of jobs re-queued.
pub async fn reap_dead_workers(pool: &Pool) -> Result<u32, RedisError> {
    let mut conn = pool.get().await?;

    // Acquire a short leader lease — only one reaper runs at a time
    let acquired: bool = redis::cmd("SET")
        .arg("claw:reaper:leader")
        .arg("1")
        .arg("NX")
        .arg("EX")
        .arg(25u64) // 25s lease, reaper runs every 30s
        .query_async(&mut *conn)
        .await
        .unwrap_or(false);

    if !acquired {
        return Ok(0); // Another reaper is running
    }

    // Get all running job IDs
    let running_ids: Vec<String> = redis::cmd("SMEMBERS")
        .arg("claw:queue:running")
        .query_async(&mut *conn)
        .await
        .unwrap_or_default();

    let mut reaped = 0u32;
    for id_str in &running_ids {
        let job_key = format!("claw:job:{}", id_str);
        let job_json: Option<String> = conn.get(&job_key).await?;
        let Some(json) = job_json else { continue };
        let job: claw_models::Job = match serde_json::from_str(&json) {
            Ok(j) => j,
            Err(_) => continue,
        };

        let Some(ref worker_id) = job.worker_id else { continue };

        // Check if worker's heartbeat is still alive.
        // The job stores the task_id (e.g. "host-1-task-0") but the heartbeat
        // is keyed by worker_id (e.g. "host-1"). Strip the "-task-N" suffix.
        let heartbeat_id = if let Some(idx) = worker_id.rfind("-task-") {
            &worker_id[..idx]
        } else {
            worker_id.as_str()
        };
        let hb_key = format!("claw:worker:{}:heartbeat", heartbeat_id);
        let exists: bool = conn.exists(&hb_key).await?;
        if exists {
            continue; // Worker is alive
        }

        // Worker is dead — atomically reap using Lua to prevent double-push
        let max_retries: u32 = 3;
        if job.retry_count >= max_retries {
            // Too many retries — mark as failed
            let mut failed_job = job.clone();
            failed_job.status = claw_models::JobStatus::Failed;
            failed_job.error = Some(format!(
                "Worker {} died and job exceeded max retries ({})",
                worker_id, max_retries
            ));
            failed_job.completed_at = Some(chrono::Utc::now());
            let updated_json = serde_json::to_string(&failed_job)?;

            // Atomic: only update if still in running set
            let script = redis::Script::new(
                r#"
                local removed = redis.call('SREM', 'claw:queue:running', ARGV[2])
                if removed == 1 then
                    redis.call('SET', KEYS[1], ARGV[1])
                    redis.call('INCR', 'claw:stats:total_failed')
                    return 1
                end
                return 0
                "#,
            );
            let result: i32 = script
                .key(&job_key)
                .arg(&updated_json)
                .arg(id_str.as_str())
                .invoke_async(&mut *conn)
                .await?;

            if result == 1 {
                tracing::warn!(job_id = %id_str, retry_count = job.retry_count, "Job exceeded max retries after worker death, marked failed");
            }
        } else {
            // Re-queue with incremented retry count
            let mut requeued_job = job.clone();
            requeued_job.status = claw_models::JobStatus::Pending;
            requeued_job.worker_id = None;
            requeued_job.started_at = None;
            requeued_job.retry_count += 1;
            let updated_json = serde_json::to_string(&requeued_job)?;
            let queue_key = format!("claw:queue:pending:{}", requeued_job.priority.min(9));

            // Atomic: only requeue if still in running set (prevents double-push)
            let script = redis::Script::new(
                r#"
                local removed = redis.call('SREM', 'claw:queue:running', ARGV[2])
                if removed == 1 then
                    redis.call('SET', KEYS[1], ARGV[1])
                    redis.call('RPUSH', KEYS[2], ARGV[2])
                    return 1
                end
                return 0
                "#,
            );
            let result: i32 = script
                .key(&job_key)
                .key(&queue_key)
                .arg(&updated_json)
                .arg(id_str.as_str())
                .invoke_async(&mut *conn)
                .await?;

            if result == 1 {
                tracing::warn!(
                    job_id = %id_str,
                    worker_id = %worker_id,
                    retry_count = requeued_job.retry_count,
                    "Reaped dead worker's job, re-queued"
                );
            }
        }

        // Release workspace lock if held
        if let Some(ws_id) = job.workspace_id {
            crate::release_workspace_lock(pool, ws_id, job.id).await.ok();
        }

        reaped += 1;
    }

    Ok(reaped)
}
