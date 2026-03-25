use axum::{
    extract::{FromRequestParts, Request, State},
    http::{request::Parts, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use std::sync::OnceLock;

use crate::AppState;

/// Authenticated user identity, extracted from session cookie.
/// Use as a handler parameter: `async fn handler(user: CurrentUser, ...) { ... }`
/// Returns 401 if no valid session. Bearer token users get username "__api_token__".
#[derive(Debug, Clone)]
pub struct CurrentUser {
    pub username: String,
    pub role: String,
}

impl FromRequestParts<AppState> for CurrentUser {
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        // Check session cookie
        let session_cookie = parts
            .headers
            .get("cookie")
            .and_then(|v| v.to_str().ok())
            .and_then(|cookies| {
                cookies.split(';').find_map(|pair| {
                    pair.trim()
                        .strip_prefix("claw_session=")
                        .map(|v| v.trim().to_string())
                })
            });

        if let Some(session_id) = session_cookie {
            if let Ok(Some(username)) = claw_redis::get_session(&state.pool, &session_id).await {
                let role = match claw_redis::get_user(&state.pool, &username).await {
                    Ok(Some(user)) => user.get("role").cloned().unwrap_or_else(|| "user".to_string()),
                    _ => "user".to_string(),
                };
                return Ok(CurrentUser { username, role });
            }
        }

        // Check bearer token
        let bearer = parts
            .headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer ").map(|t| t.to_string()));

        if let Some(token) = bearer {
            if let Some(expected) = get_api_token() {
                if token == *expected {
                    return Ok(CurrentUser {
                        username: "__api_token__".to_string(),
                        role: "admin".to_string(),
                    });
                }
            }
        }

        Err((
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "Authentication required"})),
        )
            .into_response())
    }
}

static API_TOKEN: OnceLock<Option<String>> = OnceLock::new();

fn get_api_token() -> &'static Option<String> {
    API_TOKEN.get_or_init(|| {
        let token = std::env::var("CLAW_API_TOKEN").ok().filter(|t| !t.is_empty());
        if token.is_some() {
            tracing::info!("API bearer token authentication enabled");
        }
        token
    })
}

/// Extract session ID from the `claw_session` cookie header.
fn extract_session_cookie(req: &Request) -> Option<String> {
    let cookie_header = req.headers().get("cookie")?.to_str().ok()?;
    for pair in cookie_header.split(';') {
        let pair = pair.trim();
        if let Some(value) = pair.strip_prefix("claw_session=") {
            let value = value.trim();
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

/// Extract bearer token from Authorization header or query param.
fn extract_bearer_token(req: &Request) -> Option<String> {
    // Check Authorization header
    if let Some(auth_header) = req.headers().get("authorization") {
        if let Ok(value) = auth_header.to_str() {
            if let Some(token) = value.strip_prefix("Bearer ") {
                return Some(token.to_string());
            }
        }
    }
    // Check query parameter
    if let Some(query) = req.uri().query() {
        for pair in query.split('&') {
            if let Some(token) = pair.strip_prefix("token=") {
                return Some(token.to_string());
            }
        }
    }
    None
}

/// Check if a path is exempt from authentication.
fn is_exempt_path(path: &str) -> bool {
    // Login endpoint must be accessible without auth
    if path == "/api/v1/auth/login" {
        return true;
    }
    // OAuth status is public (read-only health check)
    if path == "/api/v1/auth/oauth-status" {
        return true;
    }
    // Health check
    if path == "/api/v1/status" {
        return true;
    }
    // Static files (no /api/ prefix) are served by the SPA fallback
    if !path.starts_with("/api/") {
        return true;
    }
    false
}

pub async fn auth_middleware(
    State(state): State<AppState>,
    req: Request,
    next: Next,
) -> Response {
    let path = req.uri().path().to_string();

    // Exempt paths don't need auth
    if is_exempt_path(&path) {
        return next.run(req).await;
    }

    // 1. Check session cookie
    if let Some(session_id) = extract_session_cookie(&req) {
        match claw_redis::get_session(&state.pool, &session_id).await {
            Ok(Some(_username)) => {
                return next.run(req).await;
            }
            Ok(None) => {
                // Session expired or invalid — fall through to bearer check
            }
            Err(e) => {
                tracing::warn!(error = %e, "Session lookup failed");
                // Fall through to bearer check
            }
        }
    }

    // 2. Check bearer token
    if let Some(token) = extract_bearer_token(&req) {
        if let Some(expected) = get_api_token() {
            if token == *expected {
                return next.run(req).await;
            }
        }
    }

    // 3. If no CLAW_API_TOKEN is set and no session auth configured,
    //    check if there are any users. If no users exist, allow all
    //    (bootstrap/development mode).
    if get_api_token().is_none() {
        match claw_redis::user_count(&state.pool).await {
            Ok(0) => {
                tracing::warn!("No users and no API token configured — allowing unauthenticated access");
                return next.run(req).await;
            }
            Ok(_) => {
                // Users exist but no session/token provided — reject
            }
            Err(_) => {
                // Redis error — allow through rather than lock out during startup
                return next.run(req).await;
            }
        }
    }

    (
        StatusCode::UNAUTHORIZED,
        axum::Json(serde_json::json!({
            "error": "Authentication required. Provide a session cookie or Bearer token."
        })),
    )
        .into_response()
}

/// Bootstrap the admin user if no users exist.
/// Called once at startup.
pub async fn bootstrap_admin(pool: &deadpool_redis::Pool) {
    let username = std::env::var("CLAW_ADMIN_USER").unwrap_or_default();
    let password = std::env::var("CLAW_ADMIN_PASSWORD").unwrap_or_default();

    if username.is_empty() || password.is_empty() {
        tracing::info!("CLAW_ADMIN_USER/CLAW_ADMIN_PASSWORD not set — skipping admin bootstrap");
        return;
    }

    match claw_redis::user_count(pool).await {
        Ok(0) => {
            let hash = bcrypt::hash(&password, bcrypt::DEFAULT_COST)
                .expect("Failed to hash admin password");
            match claw_redis::create_user(pool, &username, &hash, "admin").await {
                Ok(()) => tracing::info!(username, "Admin user bootstrapped"),
                Err(e) => tracing::error!(error = %e, "Failed to bootstrap admin user"),
            }
        }
        Ok(n) => {
            tracing::info!(user_count = n, "Users already exist — skipping admin bootstrap");
        }
        Err(e) => {
            tracing::error!(error = %e, "Failed to check user count during bootstrap");
        }
    }
}
