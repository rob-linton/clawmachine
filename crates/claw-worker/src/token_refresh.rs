use deadpool_redis::redis;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// OAuth refresh endpoint and client ID for Claude Code.
const OAUTH_TOKEN_URL: &str = "https://console.anthropic.com/v1/oauth/token";
const OAUTH_CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";

/// Refresh threshold: refresh when within this many ms of expiry.
const REFRESH_THRESHOLD_MS: u64 = 3600 * 1000; // 1 hour

/// Warn when refresh token is older than this (days).
const REFRESH_TOKEN_AGE_WARN_DAYS: u64 = 75;

/// In-process mutex for coordinating refresh across worker tasks.
/// File locks (flock) coordinate across processes; this coordinates within one process.
static REFRESH_MUTEX: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn credentials_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| "/home/claw".into())
        .join(".claude")
        .join(".credentials.json")
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// Get the current OAuth token status.
/// Returns (status, expires_at_ms, refresh_token_age_days).
pub fn get_oauth_status() -> (&'static str, u64, u64) {
    let creds_path = credentials_path();
    if !creds_path.exists() {
        return ("missing", 0, 0);
    }

    let content = match std::fs::read_to_string(&creds_path) {
        Ok(c) => c,
        Err(_) => return ("missing", 0, 0),
    };
    let creds: serde_json::Value = match serde_json::from_str(&content) {
        Ok(c) => c,
        Err(_) => return ("missing", 0, 0),
    };

    let oauth = match creds.get("claudeAiOauth") {
        Some(o) => o,
        None => return ("missing", 0, 0),
    };

    let expires_at = oauth
        .get("expiresAt")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    let age_days = oauth
        .get("_refreshTokenFirstSeen")
        .and_then(|v| v.as_u64())
        .map(|first_seen| (now_ms().saturating_sub(first_seen)) / (86400 * 1000))
        .unwrap_or(0);

    let status = if expires_at > now_ms() { "valid" } else { "expired" };
    (status, expires_at, age_days)
}

/// Write current OAuth status to Redis for the UI to read.
pub async fn write_oauth_status(pool: &deadpool_redis::Pool) {
    let (status, expires_at, age_days) = get_oauth_status();
    let json = serde_json::json!({
        "status": status,
        "expires_at": expires_at,
        "refresh_token_age_days": age_days,
    });
    let mut conn = match pool.get().await {
        Ok(c) => c,
        Err(_) => return,
    };
    let _: Result<(), _> = redis::AsyncCommands::set(
        &mut *conn,
        "claw:worker:oauth_status",
        json.to_string(),
    )
    .await;
}

/// Check if OAuth token is currently valid (non-expired).
pub fn is_oauth_valid() -> bool {
    let (status, _, _) = get_oauth_status();
    status == "valid"
}

/// Check if the Claude OAuth token needs refreshing, and refresh if so.
///
/// - `api_key`: If Some, skip refresh entirely (API key mode).
/// - Returns `Ok(true)` if token was refreshed, `Ok(false)` if still valid or skipped.
/// - Returns `Err` on refresh failure (caller should warn, not crash).
pub async fn ensure_token_fresh(api_key: Option<&str>) -> Result<bool, String> {
    // API key mode — no OAuth needed
    if api_key.is_some() {
        return Ok(false);
    }

    let creds_path = credentials_path();
    if !creds_path.exists() {
        // No credentials file — might be API-key-only deployment
        return Ok(false);
    }

    // Quick check (before acquiring lock): read and parse expiry
    let content = std::fs::read_to_string(&creds_path)
        .map_err(|e| format!("Failed to read credentials: {e}"))?;
    let creds: serde_json::Value =
        serde_json::from_str(&content).map_err(|e| format!("Failed to parse credentials: {e}"))?;

    let expires_at = creds
        .get("claudeAiOauth")
        .and_then(|o| o.get("expiresAt"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    if expires_at > now_ms() + REFRESH_THRESHOLD_MS {
        return Ok(false); // Still fresh
    }

    tracing::info!(
        expires_at,
        now = now_ms(),
        "OAuth token expiring soon or expired, refreshing"
    );

    // Acquire in-process mutex (prevents concurrent refresh by parallel worker tasks)
    let _guard = REFRESH_MUTEX.lock().await;

    // Re-read after acquiring lock (another task may have refreshed while we waited)
    let content = std::fs::read_to_string(&creds_path)
        .map_err(|e| format!("Failed to re-read credentials: {e}"))?;
    let mut creds: serde_json::Value =
        serde_json::from_str(&content).map_err(|e| format!("Failed to parse credentials: {e}"))?;

    let oauth = creds
        .get("claudeAiOauth")
        .ok_or_else(|| "No claudeAiOauth in credentials".to_string())?;

    let expires_at = oauth
        .get("expiresAt")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    if expires_at > now_ms() + REFRESH_THRESHOLD_MS {
        tracing::debug!("Token already refreshed by another task");
        return Ok(false);
    }

    let refresh_token = oauth
        .get("refreshToken")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "No refreshToken in credentials".to_string())?
        .to_string();

    // Call refresh endpoint (with one retry)
    let resp = match do_refresh(&refresh_token).await {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(error = %e, "First refresh attempt failed, retrying in 5s");
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            do_refresh(&refresh_token)
                .await
                .map_err(|e2| format!("Token refresh failed after retry: {e2}"))?
        }
    };

    // Extract new tokens
    let new_access = resp
        .get("access_token")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "No access_token in refresh response".to_string())?;
    let new_refresh = resp
        .get("refresh_token")
        .and_then(|v| v.as_str())
        .unwrap_or(&refresh_token); // Some providers don't rotate
    let expires_in = resp
        .get("expires_in")
        .and_then(|v| v.as_u64())
        .unwrap_or(86400);

    let new_expires_at = now_ms() + expires_in * 1000;

    // Update credentials in memory
    if let Some(oauth_mut) = creds.get_mut("claudeAiOauth") {
        oauth_mut["accessToken"] = serde_json::Value::String(new_access.to_string());
        oauth_mut["refreshToken"] = serde_json::Value::String(new_refresh.to_string());
        oauth_mut["expiresAt"] = serde_json::json!(new_expires_at);

        // Track refresh token age (set first-seen if not already present)
        if oauth_mut.get("_refreshTokenFirstSeen").is_none() {
            oauth_mut["_refreshTokenFirstSeen"] = serde_json::json!(now_ms());
        }
    }

    // Atomic write: .tmp in same dir → rename (same filesystem required for atomicity)
    let tmp_path = creds_path.with_extension("json.tmp");
    let json = serde_json::to_string(&creds)
        .map_err(|e| format!("Failed to serialize credentials: {e}"))?;
    std::fs::write(&tmp_path, &json)
        .map_err(|e| format!("Failed to write temp credentials: {e}"))?;
    std::fs::rename(&tmp_path, &creds_path)
        .map_err(|e| format!("Failed to rename credentials: {e}"))?;

    // Check refresh token age and warn if approaching expiry
    if let Some(oauth_obj) = creds.get("claudeAiOauth") {
        if let Some(first_seen) = oauth_obj
            .get("_refreshTokenFirstSeen")
            .and_then(|v| v.as_u64())
        {
            let age_days = (now_ms() - first_seen) / (86400 * 1000);
            if age_days > REFRESH_TOKEN_AGE_WARN_DAYS {
                tracing::warn!(
                    age_days,
                    "OAuth refresh token aging — re-authenticate with `claude login` within 2 weeks"
                );
            }
        }
    }

    // _guard drops here, releasing the mutex

    tracing::info!("OAuth token refreshed successfully");
    Ok(true)
}

/// Call the OAuth token refresh endpoint.
async fn do_refresh(refresh_token: &str) -> Result<serde_json::Value, String> {
    let client = reqwest::Client::new();
    let resp = client
        .post(OAUTH_TOKEN_URL)
        .header("Content-Type", "application/json")
        .json(&serde_json::json!({
            "grant_type": "refresh_token",
            "refresh_token": refresh_token,
            "client_id": OAUTH_CLIENT_ID,
        }))
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await
        .map_err(|e| format!("HTTP request failed: {e}"))?;

    let status = resp.status();
    let body = resp
        .text()
        .await
        .map_err(|e| format!("Failed to read response: {e}"))?;

    if !status.is_success() {
        return Err(format!(
            "Refresh failed ({}): {}",
            status,
            &body[..body.len().min(200)]
        ));
    }

    serde_json::from_str(&body).map_err(|e| format!("Failed to parse response: {e}"))
}
