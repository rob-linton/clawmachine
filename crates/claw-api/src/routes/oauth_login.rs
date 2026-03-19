use axum::{
    extract::State,
    http::StatusCode,
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse,
    },
    routing::{get, post},
    Json, Router,
};
use futures::stream::Stream;
use serde::Deserialize;
use std::convert::Infallible;

use crate::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/auth/oauth-status", get(oauth_status))
        .route("/auth/oauth-login", post(oauth_login))
        .route("/auth/oauth-mfa", post(oauth_mfa))
}

/// GET /auth/oauth-status — public endpoint returning OAuth status from Redis.
async fn oauth_status(State(state): State<AppState>) -> impl IntoResponse {
    use redis::AsyncCommands;
    let mut conn = match state.pool.get().await {
        Ok(c) => c,
        Err(e) => {
            tracing::error!(error = %e, "Redis connection failed for oauth-status");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "Redis connection failed"})),
            )
                .into_response();
        }
    };

    let value: Option<String> = conn
        .get("claw:worker:oauth_status")
        .await
        .unwrap_or(None);

    match value {
        Some(json_str) => {
            // Return the raw JSON stored in Redis
            match serde_json::from_str::<serde_json::Value>(&json_str) {
                Ok(val) => (StatusCode::OK, Json(val)).into_response(),
                Err(_) => (
                    StatusCode::OK,
                    Json(serde_json::json!({"status": "unknown"})),
                )
                    .into_response(),
            }
        }
        None => (
            StatusCode::OK,
            Json(serde_json::json!({"status": "unknown"})),
        )
            .into_response(),
    }
}

#[derive(Deserialize)]
struct OAuthLoginRequest {
    email: String,
    password: String,
}

/// POST /auth/oauth-login — encrypt password, publish request, return SSE stream.
async fn oauth_login(
    State(state): State<AppState>,
    Json(req): Json<OAuthLoginRequest>,
) -> impl IntoResponse {
    use redis::AsyncCommands;

    // Get encryption key
    let secret_key = match std::env::var("CLAW_SECRET_KEY") {
        Ok(k) if !k.is_empty() => k,
        _ => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "CLAW_SECRET_KEY not configured"})),
            )
                .into_response();
        }
    };

    let encryption_key = derive_key(&secret_key);
    let encrypted_password = match encrypt_value(&encryption_key, &req.password) {
        Ok(enc) => enc,
        Err(e) => {
            tracing::error!(error = %e, "Failed to encrypt password");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "Encryption failed"})),
            )
                .into_response();
        }
    };

    let request_id = uuid::Uuid::new_v4().to_string();

    // Publish login request to Redis
    let payload = serde_json::json!({
        "email": req.email,
        "encrypted_password": encrypted_password,
        "request_id": request_id,
    });

    let mut conn = match state.pool.get().await {
        Ok(c) => c,
        Err(e) => {
            tracing::error!(error = %e, "Redis connection failed for oauth-login");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "Redis connection failed"})),
            )
                .into_response();
        }
    };

    if let Err(e) = conn
        .publish::<_, _, ()>("claw:oauth-login:request", payload.to_string())
        .await
    {
        tracing::error!(error = %e, "Failed to publish oauth login request");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "Failed to publish request"})),
        )
            .into_response();
    }

    tracing::info!(request_id, "OAuth login request published");

    // Return SSE stream subscribing to progress channel
    let stream = oauth_login_stream(state.redis_url, request_id);
    Sse::new(stream).keep_alive(KeepAlive::default()).into_response()
}

fn oauth_login_stream(
    redis_url: String,
    request_id: String,
) -> impl Stream<Item = Result<Event, Infallible>> {
    async_stream::stream! {
        let channel = format!("claw:oauth-login:progress:{}", request_id);

        let client = match redis::Client::open(redis_url.as_str()) {
            Ok(c) => c,
            Err(e) => {
                tracing::error!(error = %e, "Failed to create Redis client for OAuth SSE");
                yield Ok(Event::default().data(
                    serde_json::json!({"step":"error","message":"Redis connection failed"}).to_string()
                ));
                return;
            }
        };

        let mut pubsub = match client.get_async_pubsub().await {
            Ok(ps) => ps,
            Err(e) => {
                tracing::error!(error = %e, "Failed to create PubSub for OAuth SSE");
                yield Ok(Event::default().data(
                    serde_json::json!({"step":"error","message":"PubSub failed"}).to_string()
                ));
                return;
            }
        };

        if let Err(e) = pubsub.subscribe(&channel).await {
            tracing::error!(error = %e, "Failed to subscribe to OAuth progress");
            return;
        }

        tracing::debug!(request_id, "SSE client connected to OAuth progress");
        yield Ok(Event::default().event("connected").data(
            serde_json::json!({"request_id": request_id}).to_string()
        ));

        loop {
            use futures::StreamExt;
            match pubsub.on_message().next().await {
                Some(msg) => {
                    if let Ok(payload) = msg.get_payload::<String>() {
                        // Forward the progress event
                        yield Ok(Event::default().event("oauth_progress").data(payload.clone()));

                        // Check if this is a terminal event
                        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&payload) {
                            if let Some(step) = parsed.get("step").and_then(|s| s.as_str()) {
                                if step == "success" || step == "error" {
                                    break;
                                }
                            }
                        }
                    }
                }
                None => {
                    tracing::debug!("OAuth SSE PubSub stream ended");
                    break;
                }
            }
        }
    }
}

#[derive(Deserialize)]
struct OAuthMfaRequest {
    request_id: String,
    code: String,
}

/// POST /auth/oauth-mfa — publish MFA code to the waiting handler.
async fn oauth_mfa(
    State(state): State<AppState>,
    Json(req): Json<OAuthMfaRequest>,
) -> impl IntoResponse {
    use redis::AsyncCommands;

    let mut conn = match state.pool.get().await {
        Ok(c) => c,
        Err(e) => {
            tracing::error!(error = %e, "Redis connection failed for oauth-mfa");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "Redis connection failed"})),
            )
                .into_response();
        }
    };

    let channel = format!("claw:oauth-login:mfa:{}", req.request_id);
    if let Err(e) = conn.publish::<_, _, ()>(&channel, &req.code).await {
        tracing::error!(error = %e, "Failed to publish MFA code");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "Failed to publish MFA code"})),
        )
            .into_response();
    }

    tracing::info!(request_id = %req.request_id, "MFA code published");
    (StatusCode::OK, Json(serde_json::json!({"status": "submitted"}))).into_response()
}

// --- AES-256-GCM encryption (same pattern as claw-redis/src/credentials.rs) ---

use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use rand::RngCore;

/// Derive a 32-byte key from the CLAW_SECRET_KEY string.
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

/// Encrypt a value using AES-256-GCM. Returns base64(nonce || ciphertext).
fn encrypt_value(key: &[u8; 32], plaintext: &str) -> Result<String, String> {
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|e| format!("Cipher init: {e}"))?;
    let mut nonce_bytes = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, plaintext.as_bytes())
        .map_err(|e| format!("Encrypt failed: {e}"))?;

    let mut combined = Vec::with_capacity(12 + ciphertext.len());
    combined.extend_from_slice(&nonce_bytes);
    combined.extend_from_slice(&ciphertext);

    Ok(BASE64.encode(&combined))
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
