use deadpool_redis::Pool;
use redis::AsyncCommands;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Background loop that listens for OAuth login requests via Redis pub/sub
/// and orchestrates the Puppeteer-based login flow.
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

    tracing::debug!("Subscribed to claw:oauth-login:request");

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
            Ok(None) => {
                return Err("PubSub stream ended".into());
            }
            Err(_) => {
                // Timeout — loop back to check shutdown flag
                continue;
            }
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
    let encrypted_password = match parsed.get("encrypted_password").and_then(|v| v.as_str()) {
        Some(p) => p.to_string(),
        None => {
            tracing::error!("Missing encrypted_password in login request");
            return;
        }
    };
    let request_id = match parsed.get("request_id").and_then(|v| v.as_str()) {
        Some(r) => r.to_string(),
        None => {
            tracing::error!("Missing request_id in login request");
            return;
        }
    };

    let progress_channel = format!("claw:oauth-login:progress:{}", request_id);

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
            .arg(&request_id)
            .arg("NX")
            .arg("EX")
            .arg(600)
            .query_async(&mut *conn)
            .await
            .unwrap_or(false);

        if !locked {
            publish_progress(pool, &progress_channel, "error", "Another OAuth login is already in progress").await;
            return;
        }
    }

    tracing::info!(request_id, email = %email, "Processing OAuth login request");

    // Decrypt password
    let password = match decrypt_password(&encrypted_password) {
        Ok(p) => p,
        Err(e) => {
            tracing::error!(error = %e, "Failed to decrypt password");
            publish_progress(pool, &progress_channel, "error", &format!("Decryption failed: {e}")).await;
            cleanup_lock(pool).await;
            return;
        }
    };

    publish_progress(pool, &progress_channel, "starting", "Starting OAuth login...").await;

    // Step 1: Logout existing session (ignore errors)
    let _ = tokio::process::Command::new("claude")
        .args(["auth", "logout"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .status()
        .await;

    // Step 2: Start `claude auth login`
    publish_progress(pool, &progress_channel, "authenticating", "Running claude auth login...").await;

    let login_output = match tokio::process::Command::new("claude")
        .args(["auth", "login", "--email", &email])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .await
    {
        Ok(o) => o,
        Err(e) => {
            publish_progress(pool, &progress_channel, "error", &format!("Failed to run claude auth login: {e}")).await;
            cleanup_lock(pool).await;
            return;
        }
    };

    let stdout = String::from_utf8_lossy(&login_output.stdout);
    let stderr = String::from_utf8_lossy(&login_output.stderr);
    let combined = format!("{}\n{}", stdout, stderr);

    // Parse OAuth URL from output
    let oauth_url = combined
        .lines()
        .find(|line| line.contains("https://") && line.to_lowercase().contains("oauth"))
        .or_else(|| combined.lines().find(|line| line.contains("https://")))
        .map(|line| {
            // Extract URL from the line
            line.split_whitespace()
                .find(|word| word.starts_with("https://"))
                .unwrap_or(line.trim())
                .to_string()
        });

    let oauth_url = match oauth_url {
        Some(url) => url,
        None => {
            let msg = format!("Could not find OAuth URL in claude output. stdout: {}, stderr: {}", stdout.chars().take(200).collect::<String>(), stderr.chars().take(200).collect::<String>());
            publish_progress(pool, &progress_channel, "error", &msg).await;
            cleanup_lock(pool).await;
            return;
        }
    };

    publish_progress(pool, &progress_channel, "navigating", "Got OAuth URL, launching browser...").await;

    // Step 3: Run Puppeteer script
    let scripts_dir = std::env::var("CLAW_SCRIPTS_DIR")
        .unwrap_or_else(|_| "/app/scripts".to_string());
    let script_path = std::path::PathBuf::from(&scripts_dir).join("oauth-login.js");

    let mut child = match tokio::process::Command::new("node")
        .arg(&script_path)
        .env("OAUTH_URL", &oauth_url)
        .env("OAUTH_EMAIL", &email)
        .env("OAUTH_PASSWORD", &password)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            publish_progress(pool, &progress_channel, "error", &format!("Failed to spawn puppeteer script: {e}")).await;
            cleanup_lock(pool).await;
            return;
        }
    };

    let child_stdin = child.stdin.take();
    let child_stdout = child.stdout.take();

    // Read stdout line by line and forward as progress events
    if let Some(stdout) = child_stdout {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
        let mut reader = BufReader::new(stdout).lines();
        let mfa_channel = format!("claw:oauth-login:mfa:{}", request_id);
        let mut child_stdin = child_stdin;

        while let Ok(Some(line)) = reader.next_line().await {
            // Publish each line as a progress event
            {
                let mut conn = match pool.get().await {
                    Ok(c) => c,
                    Err(_) => continue,
                };
                let _: Result<(), _> = conn
                    .publish::<_, _, ()>(&progress_channel, &line)
                    .await;
            }

            // Check if MFA is required
            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&line) {
                if parsed.get("step").and_then(|s| s.as_str()) == Some("mfa_required") {
                    // Subscribe to MFA channel and wait for code
                    if let Some(code) = wait_for_mfa(&mfa_channel).await {
                        // Write MFA code to script's stdin
                        if let Some(ref mut stdin) = child_stdin {
                            let _ = stdin.write_all(format!("{}\n", code).as_bytes()).await;
                            let _ = stdin.flush().await;
                        }
                    } else {
                        // MFA timeout
                        publish_progress(pool, &progress_channel, "error", "MFA code timeout").await;
                        let _ = child.kill().await;
                        cleanup_lock(pool).await;
                        return;
                    }
                }
            }
        }
    }

    // Wait for script to exit
    let exit_status = match child.wait().await {
        Ok(s) => s,
        Err(e) => {
            publish_progress(pool, &progress_channel, "error", &format!("Script wait failed: {e}")).await;
            cleanup_lock(pool).await;
            return;
        }
    };

    if exit_status.success() {
        // Verify credentials file exists
        let creds_path = dirs::home_dir()
            .unwrap_or_else(|| "/home/claw".into())
            .join(".claude")
            .join(".credentials.json");

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
                        publish_progress(pool, &progress_channel, "success", "OAuth login completed successfully").await;
                    } else {
                        publish_progress(pool, &progress_channel, "error", "Credentials file found but token is expired").await;
                    }
                } else {
                    publish_progress(pool, &progress_channel, "error", "Credentials file is not valid JSON").await;
                }
            } else {
                publish_progress(pool, &progress_channel, "error", "Cannot read credentials file").await;
            }
        } else {
            publish_progress(pool, &progress_channel, "error", "Credentials file not found after login").await;
        }
    } else {
        let code = exit_status.code().unwrap_or(-1);
        publish_progress(pool, &progress_channel, "error", &format!("Login script exited with code {code}")).await;
    }

    // Update OAuth status in Redis
    write_oauth_status(pool).await;

    // Cleanup lock
    cleanup_lock(pool).await;
}

async fn publish_progress(pool: &Pool, channel: &str, step: &str, message: &str) {
    let payload = serde_json::json!({
        "step": step,
        "message": message,
    });
    if let Ok(mut conn) = pool.get().await {
        let _: Result<(), _> = conn
            .publish::<_, _, ()>(channel, payload.to_string())
            .await;
    }
}

async fn cleanup_lock(pool: &Pool) {
    if let Ok(mut conn) = pool.get().await {
        let _: Result<(), _> = conn.del("claw:oauth-login:active").await;
    }
}

async fn wait_for_mfa(channel: &str) -> Option<String> {
    let redis_url = std::env::var("CLAW_REDIS_URL")
        .unwrap_or_else(|_| "redis://127.0.0.1:6379".into());

    let client = redis::Client::open(redis_url.as_str()).ok()?;
    let mut pubsub = client.get_async_pubsub().await.ok()?;
    pubsub.subscribe(channel).await.ok()?;

    use futures::StreamExt;
    let mut stream = pubsub.on_message();
    let result = match tokio::time::timeout(
        std::time::Duration::from_secs(300), // 5 minutes
        stream.next(),
    )
    .await
    {
        Ok(Some(msg)) => msg.get_payload::<String>().ok(),
        _ => None,
    };
    drop(stream);
    result
}

/// Write current OAuth status to Redis (mirrors token_refresh::write_oauth_status).
async fn write_oauth_status(pool: &Pool) {
    let creds_path = dirs::home_dir()
        .unwrap_or_else(|| "/home/claw".into())
        .join(".claude")
        .join(".credentials.json");

    let status = if !creds_path.exists() {
        serde_json::json!({"status": "missing", "expires_at": 0, "refresh_token_age_days": 0})
    } else if let Ok(content) = std::fs::read_to_string(&creds_path) {
        if let Ok(creds) = serde_json::from_str::<serde_json::Value>(&content) {
            let oauth = creds.get("claudeAiOauth");
            let expires_at = oauth
                .and_then(|o| o.get("expiresAt"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
            let age_days = oauth
                .and_then(|o| o.get("_refreshTokenFirstSeen"))
                .and_then(|v| v.as_u64())
                .map(|first_seen| now_ms.saturating_sub(first_seen) / (86400 * 1000))
                .unwrap_or(0);
            let status_str = if expires_at > now_ms { "valid" } else { "expired" };
            serde_json::json!({
                "status": status_str,
                "expires_at": expires_at,
                "refresh_token_age_days": age_days,
            })
        } else {
            serde_json::json!({"status": "missing", "expires_at": 0, "refresh_token_age_days": 0})
        }
    } else {
        serde_json::json!({"status": "missing", "expires_at": 0, "refresh_token_age_days": 0})
    };

    if let Ok(mut conn) = pool.get().await {
        let _: Result<(), _> = conn
            .set::<_, _, ()>("claw:worker:oauth_status", status.to_string())
            .await;
    }
}

// --- AES-256-GCM decryption (same pattern as claw-redis/src/credentials.rs) ---

fn decrypt_password(encoded: &str) -> Result<String, String> {
    let secret_key = std::env::var("CLAW_SECRET_KEY")
        .map_err(|_| "CLAW_SECRET_KEY not set".to_string())?;

    let key = derive_key(&secret_key);
    decrypt_value(&key, encoded)
}

fn derive_key(key_str: &str) -> [u8; 32] {
    let key_str = key_str.trim();

    // Try hex decode first (64 hex chars = 32 bytes)
    if key_str.len() == 64 {
        if let Ok(bytes) = hex_decode(key_str) {
            if bytes.len() == 32 {
                let mut key = [0u8; 32];
                key.copy_from_slice(&bytes);
                return key;
            }
        }
    }

    // Try base64 decode
    use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
    if let Ok(bytes) = BASE64.decode(key_str) {
        if bytes.len() == 32 {
            let mut key = [0u8; 32];
            key.copy_from_slice(&bytes);
            return key;
        }
    }

    // Fall back to SHA-256 hash
    use sha2::{Digest, Sha256};
    let hash = Sha256::digest(key_str.as_bytes());
    let mut key = [0u8; 32];
    key.copy_from_slice(&hash);
    key
}

fn decrypt_value(key: &[u8; 32], encoded: &str) -> Result<String, String> {
    use aes_gcm::{
        aead::{Aead, KeyInit},
        Aes256Gcm, Nonce,
    };
    use base64::{engine::general_purpose::STANDARD as BASE64, Engine};

    let combined = BASE64
        .decode(encoded)
        .map_err(|e| format!("Base64 decode: {e}"))?;
    if combined.len() < 13 {
        return Err("Ciphertext too short".into());
    }

    let (nonce_bytes, ciphertext) = combined.split_at(12);
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|e| format!("Cipher init: {e}"))?;
    let nonce = Nonce::from_slice(nonce_bytes);

    let plaintext = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|e| format!("Decrypt failed: {e}"))?;

    String::from_utf8(plaintext).map_err(|e| format!("UTF-8 decode: {e}"))
}

fn hex_decode(s: &str) -> Result<Vec<u8>, String> {
    if s.len() % 2 != 0 {
        return Err("Odd length".into());
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(|e| e.to_string()))
        .collect()
}
