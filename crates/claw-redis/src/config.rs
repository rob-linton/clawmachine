use deadpool_redis::Pool;
use redis::AsyncCommands;
use std::collections::HashMap;

use crate::RedisError;

const CONFIG_PREFIX: &str = "claw:config:";

/// Default config values applied when no value is stored.
fn defaults() -> HashMap<&'static str, &'static str> {
    let mut m = HashMap::new();
    m.insert("execution_backend", "local");
    m.insert("sandbox_image", "claw-sandbox:latest");
    m.insert("docker_memory_limit", "4g");
    m.insert("docker_cpu_limit", "2.0");
    m.insert("docker_pids_limit", "256");
    m.insert("repos_dir", "~/.claw/repos");
    m.insert("checkouts_dir", "~/.claw/checkouts");
    // Default credential mounts: Claude OAuth + gh CLI
    m.insert(
        "docker_credential_mounts",
        r#"[{"host_path":"~/.claude","container_path":"/home/claw/.claude","readonly":false},{"host_path":"~/.claude.json","container_path":"/home/claw/.claude.json","readonly":true},{"host_path":"~/.config/gh","container_path":"/home/claw/.config/gh","readonly":true},{"host_path":"~/.ssh","container_path":"/home/claw/.ssh","readonly":true}]"#,
    );
    m
}

/// Get a single config value. Returns default if not set.
pub async fn get_config(pool: &Pool, key: &str) -> Result<String, RedisError> {
    let mut conn = pool.get().await?;
    let redis_key = format!("{}{}", CONFIG_PREFIX, key);
    let val: Option<String> = conn.get(&redis_key).await?;
    match val {
        Some(v) => Ok(v),
        None => Ok(defaults()
            .get(key)
            .map(|s| s.to_string())
            .unwrap_or_default()),
    }
}

/// Set a single config value.
pub async fn set_config(pool: &Pool, key: &str, value: &str) -> Result<(), RedisError> {
    let mut conn = pool.get().await?;
    let redis_key = format!("{}{}", CONFIG_PREFIX, key);
    let _: () = conn.set(&redis_key, value).await?;
    tracing::info!(key, "Config updated");
    Ok(())
}

/// Get all config as a JSON-compatible map. Merges stored values over defaults.
pub async fn get_all_config(pool: &Pool) -> Result<HashMap<String, String>, RedisError> {
    let mut conn = pool.get().await?;
    let mut result: HashMap<String, String> = defaults()
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();

    // Scan for all stored config keys and overlay
    let pattern = format!("{}*", CONFIG_PREFIX);
    let keys: Vec<String> = redis::cmd("KEYS")
        .arg(&pattern)
        .query_async(&mut *conn)
        .await
        .unwrap_or_default();

    for key in &keys {
        if let Ok(val) = conn.get::<_, String>(key).await {
            let short_key = key.strip_prefix(CONFIG_PREFIX).unwrap_or(key);
            result.insert(short_key.to_string(), val);
        }
    }

    Ok(result)
}

/// Update multiple config values at once (partial merge).
pub async fn set_configs(pool: &Pool, values: &HashMap<String, String>) -> Result<(), RedisError> {
    let mut conn = pool.get().await?;
    let mut pipe = redis::pipe();
    for (key, value) in values {
        let redis_key = format!("{}{}", CONFIG_PREFIX, key);
        pipe.set(&redis_key, value);
    }
    pipe.exec_async(&mut *conn).await?;
    tracing::info!(count = values.len(), "Config bulk updated");
    Ok(())
}
