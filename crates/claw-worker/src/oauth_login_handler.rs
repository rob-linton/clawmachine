use deadpool_redis::Pool;
use redis::AsyncCommands;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Background loop that listens for OAuth login requests via Redis pub/sub.
/// When triggered, runs `claude auth login` and provides the OAuth URL to the UI.
/// The user completes the login in their own browser.
pub async fn oauth_login_loop(pool: Pool, shutdown: Arc<AtomicBool>) {
    tracing::info!("OAuth login handler started");

    let redis_url = std::env::var("CLAW_REDIS_URL")
        .unwrap_or_else(|_| "redis://127.0.0.1:6379".into());

    loop {
        if shutdown.load(Ordering::Relaxed) {
            break;
        }

        if let Err(e) = run_subscriber(&redis_url, &pool, &shutdown).await {
            tracing::warn!(error = %e, "OAuth login subscriber error, reconnecting in 5s");
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        }
    }

    tracing::info!("OAuth login handler stopped");
}

async fn run_subscriber(
    redis_url: &str,
    pool: &Pool,
    shutdown: &Arc<AtomicBool>,
) -> Result<(), String> {
    let client = redis::Client::open(redis_url)
        .map_err(|e| format!("Redis client: {e}"))?;
    let mut pubsub = client
        .get_async_pubsub()
        .await
        .map_err(|e| format!("PubSub: {e}"))?;

    pubsub
        .subscribe("claw:oauth-login:request")
        .await
        .map_err(|e| format!("Subscribe: {e}"))?;

    loop {
        if shutdown.load(Ordering::Relaxed) {
            break;
        }

        use futures::StreamExt;
        match tokio::time::timeout(
            std::time::Duration::from_secs(5),
            pubsub.on_message().next(),
        )
        .await
        {
            Ok(Some(msg)) => {
                let payload: String = match msg.get_payload() {
                    Ok(p) => p,
                    Err(e) => {
                        tracing::warn!(error = %e, "Bad pubsub payload");
                        continue;
                    }
                };
                handle_login_request(pool, &payload).await;
            }
            Ok(None) => return Err("PubSub stream ended".into()),
            Err(_) => continue, // Timeout — check shutdown flag
        }
    }

    Ok(())
}

async fn handle_login_request(pool: &Pool, payload: &str) {
    let parsed: serde_json::Value = match serde_json::from_str(payload) {
        Ok(v) => v,
        Err(e) => {
            tracing::error!(error = %e, "Failed to parse login request");
            return;
        }
    };

    let email = match parsed.get("email").and_then(|v| v.as_str()) {
        Some(e) => e.to_string(),
        None => {
            tracing::error!("Missing email in login request");
            return;
        }
    };

    // Acquire lock (only one login at a time)
    {
        let mut conn = match pool.get().await {
            Ok(c) => c,
            Err(e) => {
                tracing::error!(error = %e, "Redis connection failed for OAuth lock");
                return;
            }
        };

        let locked: bool = redis::cmd("SET")
            .arg("claw:oauth-login:active")
            .arg("1")
            .arg("NX")
            .arg("EX")
            .arg(600)
            .query_async(&mut *conn)
            .await
            .unwrap_or(false);

        if !locked {
            tracing::warn!("Another OAuth login is already in progress");
            return;
        }
    }

    tracing::info!(email = %email, "Processing OAuth login request");

    // Kill any stale claude auth processes
    let _ = tokio::process::Command::new("pkill")
        .args(["-f", "claude auth"])
        .status()
        .await;

    // Delete old credentials for clean state
    let creds_path = dirs::home_dir()
        .unwrap_or_else(|| "/home/claw".into())
        .join(".claude")
        .join(".credentials.json");
    let _ = tokio::fs::remove_file(&creds_path).await;

    // Spawn claude auth login
    let mut login_proc = match tokio::process::Command::new("claude")
        .args(["auth", "login", "--email", &email])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
    {
        Ok(p) => p,
        Err(e) => {
            tracing::error!(error = %e, "Failed to spawn claude auth login");
            write_status(pool, "error", None).await;
            cleanup_lock(pool).await;
            return;
        }
    };

    // Read stdout to find the OAuth URL
    let login_stdout = login_proc.stdout.take();
    let mut oauth_url: Option<String> = None;

    if let Some(stdout) = login_stdout {
        use tokio::io::{AsyncBufReadExt, BufReader};
        let mut reader = BufReader::new(stdout).lines();
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(15);
        loop {
            match tokio::time::timeout_at(deadline, reader.next_line()).await {
                Ok(Ok(Some(line))) => {
                    tracing::debug!(line = %line, "claude auth login output");
                    if line.contains("https://") {
                        let url = line
                            .split_whitespace()
                            .find(|w| w.starts_with("https://"))
                            .unwrap_or(line.trim())
                            .to_string();
                        oauth_url = Some(url);
                        break;
                    }
                }
                _ => break,
            }
        }
    }

    let oauth_url = match oauth_url {
        Some(url) => url,
        None => {
            tracing::error!("Could not find OAuth URL from claude auth login");
            write_status(pool, "error", None).await;
            let _ = login_proc.kill().await;
            cleanup_lock(pool).await;
            return;
        }
    };

    tracing::info!(oauth_url = %oauth_url, "OAuth URL ready for user");

    // Write URL to status so the UI can display it
    write_status(pool, "login_in_progress", Some(&oauth_url)).await;

    // Poll for credentials file (up to 10 minutes)
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(600);

    loop {
        // Check if claude auth login exited (got the callback)
        match login_proc.try_wait() {
            Ok(Some(status)) => {
                tracing::info!(exit_code = ?status.code(), "claude auth login completed");
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                break;
            }
            Ok(None) => {} // Still running
            Err(e) => {
                tracing::warn!(error = %e, "Error checking claude auth login");
                break;
            }
        }

        // Check if credentials appeared
        if creds_path.exists() {
            if let Ok(content) = tokio::fs::read_to_string(&creds_path).await {
                if let Ok(creds) = serde_json::from_str::<serde_json::Value>(&content) {
                    let expires_at = creds
                        .get("claudeAiOauth")
                        .and_then(|o| o.get("expiresAt"))
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0);
                    let now_ms = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis() as u64;
                    if expires_at > now_ms {
                        tracing::info!("OAuth credentials obtained successfully");
                        break;
                    }
                }
            }
        }

        if tokio::time::Instant::now() > deadline {
            tracing::warn!("OAuth login timed out after 10 minutes");
            break;
        }

        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    }

    // Kill claude auth login if still running
    let _ = login_proc.kill().await;

    // Check final result
    let success = if creds_path.exists() {
        tokio::fs::read_to_string(&creds_path)
            .await
            .ok()
            .and_then(|c| serde_json::from_str::<serde_json::Value>(&c).ok())
            .and_then(|c| c.get("claudeAiOauth")?.get("expiresAt")?.as_u64())
            .map(|exp| {
                exp > std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64
            })
            .unwrap_or(false)
    } else {
        false
    };

    if success {
        tracing::info!("OAuth login completed successfully");
        // Write fresh status (token_refresh will update it on next cycle too)
        crate::token_refresh::write_oauth_status(pool).await;
    } else {
        tracing::error!("OAuth login failed — credentials not obtained");
        write_status(pool, "expired", None).await;
    }

    cleanup_lock(pool).await;
}

async fn write_status(pool: &Pool, status: &str, oauth_url: Option<&str>) {
    let mut json = serde_json::json!({
        "status": status,
        "expires_at": 0,
        "refresh_token_age_days": 0,
    });
    if let Some(url) = oauth_url {
        json["oauth_url"] = serde_json::Value::String(url.to_string());
    }
    if let Ok(mut conn) = pool.get().await {
        let _: Result<(), _> = conn
            .set::<_, _, ()>("claw:worker:oauth_status", json.to_string())
            .await;
    }
}

async fn cleanup_lock(pool: &Pool) {
    if let Ok(mut conn) = pool.get().await {
        let _: Result<(), _> = conn.del("claw:oauth-login:active").await;
    }
}
