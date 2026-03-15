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
