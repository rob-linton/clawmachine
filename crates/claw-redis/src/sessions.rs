use deadpool_redis::Pool;
use redis::AsyncCommands;
use uuid::Uuid;

use crate::RedisError;

const SESSION_PREFIX: &str = "claw:session:";
const SESSION_TTL_SECS: u64 = 86400; // 24 hours

/// Create a new session for a user. Returns the session ID.
pub async fn create_session(pool: &Pool, username: &str) -> Result<String, RedisError> {
    let mut conn = pool.get().await?;
    let session_id = Uuid::new_v4().to_string();
    let key = format!("{}{}", SESSION_PREFIX, session_id);

    redis::pipe()
        .hset(&key, "username", username)
        .hset(&key, "created_at", chrono::Utc::now().to_rfc3339())
        .exec_async(&mut *conn)
        .await?;

    // Set TTL
    let _: () = conn.expire(&key, SESSION_TTL_SECS as i64).await?;

    tracing::debug!(username, session_id = %session_id, "Session created");
    Ok(session_id)
}

/// Look up a session. Returns the username if valid, None if expired or not found.
pub async fn get_session(pool: &Pool, session_id: &str) -> Result<Option<String>, RedisError> {
    let mut conn = pool.get().await?;
    let key = format!("{}{}", SESSION_PREFIX, session_id);
    let username: Option<String> = conn.hget(&key, "username").await?;
    Ok(username)
}

/// Delete a session (logout).
pub async fn delete_session(pool: &Pool, session_id: &str) -> Result<(), RedisError> {
    let mut conn = pool.get().await?;
    let key = format!("{}{}", SESSION_PREFIX, session_id);
    let _: () = conn.del(&key).await?;
    tracing::debug!(session_id, "Session deleted");
    Ok(())
}
