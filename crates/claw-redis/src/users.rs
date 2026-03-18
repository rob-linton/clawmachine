use deadpool_redis::Pool;
use redis::AsyncCommands;

use crate::RedisError;

const USER_PREFIX: &str = "claw:user:";

/// Create a new user with a pre-hashed password.
pub async fn create_user(
    pool: &Pool,
    username: &str,
    password_hash: &str,
    role: &str,
) -> Result<(), RedisError> {
    let mut conn = pool.get().await?;
    let key = format!("{}{}", USER_PREFIX, username);

    // Check if user already exists
    let exists: bool = conn.exists(&key).await?;
    if exists {
        return Err(RedisError::Redis(redis::RedisError::from((
            redis::ErrorKind::ExtensionError,
            "User already exists",
            username.to_string(),
        ))));
    }

    redis::pipe()
        .hset(&key, "password_hash", password_hash)
        .hset(&key, "role", role)
        .hset(&key, "created_at", chrono::Utc::now().to_rfc3339())
        .exec_async(&mut *conn)
        .await?;

    tracing::info!(username, role, "User created");
    Ok(())
}

/// Get a user's fields (password_hash, role, created_at). Returns None if not found.
pub async fn get_user(
    pool: &Pool,
    username: &str,
) -> Result<Option<std::collections::HashMap<String, String>>, RedisError> {
    let mut conn = pool.get().await?;
    let key = format!("{}{}", USER_PREFIX, username);
    let exists: bool = conn.exists(&key).await?;
    if !exists {
        return Ok(None);
    }
    let fields: std::collections::HashMap<String, String> =
        conn.hgetall(&key).await?;
    if fields.is_empty() {
        return Ok(None);
    }
    Ok(Some(fields))
}

/// Update a user's password hash.
pub async fn update_user_password(
    pool: &Pool,
    username: &str,
    password_hash: &str,
) -> Result<(), RedisError> {
    let mut conn = pool.get().await?;
    let key = format!("{}{}", USER_PREFIX, username);
    let exists: bool = conn.exists(&key).await?;
    if !exists {
        return Err(RedisError::Redis(redis::RedisError::from((
            redis::ErrorKind::ExtensionError,
            "User not found",
            username.to_string(),
        ))));
    }
    let _: () = conn.hset(&key, "password_hash", password_hash).await?;
    tracing::info!(username, "User password updated");
    Ok(())
}

/// List all users (returns vec of (username, role) pairs).
pub async fn list_users(pool: &Pool) -> Result<Vec<(String, String)>, RedisError> {
    let mut conn = pool.get().await?;
    let pattern = format!("{}*", USER_PREFIX);
    let keys: Vec<String> = redis::cmd("KEYS")
        .arg(&pattern)
        .query_async(&mut *conn)
        .await
        .unwrap_or_default();

    let mut users = Vec::new();
    for key in &keys {
        let role: String = conn.hget(key, "role").await.unwrap_or_default();
        let username = key.strip_prefix(USER_PREFIX).unwrap_or(key).to_string();
        users.push((username, role));
    }
    users.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(users)
}

/// Delete a user.
pub async fn delete_user(pool: &Pool, username: &str) -> Result<(), RedisError> {
    let mut conn = pool.get().await?;
    let key = format!("{}{}", USER_PREFIX, username);
    let deleted: i64 = conn.del(&key).await?;
    if deleted == 0 {
        return Err(RedisError::Redis(redis::RedisError::from((
            redis::ErrorKind::ExtensionError,
            "User not found",
            username.to_string(),
        ))));
    }
    tracing::info!(username, "User deleted");
    Ok(())
}

/// Count the number of users.
pub async fn user_count(pool: &Pool) -> Result<u64, RedisError> {
    let mut conn = pool.get().await?;
    let pattern = format!("{}*", USER_PREFIX);
    let keys: Vec<String> = redis::cmd("KEYS")
        .arg(&pattern)
        .query_async(&mut *conn)
        .await
        .unwrap_or_default();
    Ok(keys.len() as u64)
}
