use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post, delete},
    Json, Router,
};
use serde::Deserialize;

use crate::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/auth/login", post(login))
        .route("/auth/logout", post(logout))
        .route("/auth/me", get(me))
        .route("/auth/users", post(create_user).get(list_users))
        .route("/auth/users/{username}", delete(delete_user))
}

#[derive(Deserialize)]
struct LoginRequest {
    username: String,
    password: String,
}

async fn login(
    State(state): State<AppState>,
    Json(req): Json<LoginRequest>,
) -> impl IntoResponse {
    // Look up user
    let user = match claw_redis::get_user(&state.pool, &req.username).await {
        Ok(Some(u)) => u,
        Ok(None) => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({"error": "Invalid username or password"})),
            )
                .into_response();
        }
        Err(e) => {
            tracing::error!(error = %e, "Failed to look up user");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "Internal error"})),
            )
                .into_response();
        }
    };

    // Verify password
    let hash = user.get("password_hash").cloned().unwrap_or_default();
    let valid = bcrypt::verify(&req.password, &hash).unwrap_or(false);
    if !valid {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "Invalid username or password"})),
        )
            .into_response();
    }

    // Create session
    let session_id = match claw_redis::create_session(&state.pool, &req.username).await {
        Ok(id) => id,
        Err(e) => {
            tracing::error!(error = %e, "Failed to create session");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "Internal error"})),
            )
                .into_response();
        }
    };

    // Set cookie — only add Secure flag when behind HTTPS
    let secure_flag = if std::env::var("CLAW_CORS_ORIGIN")
        .unwrap_or_default()
        .starts_with("https")
    {
        "; Secure"
    } else {
        ""
    };
    let cookie = format!(
        "claw_session={}; HttpOnly; SameSite=Lax; Path=/; Max-Age=86400{}",
        session_id, secure_flag
    );

    let mut headers = HeaderMap::new();
    headers.insert("set-cookie", cookie.parse().unwrap());

    let role = user.get("role").cloned().unwrap_or_default();
    (
        StatusCode::OK,
        headers,
        Json(serde_json::json!({
            "username": req.username,
            "role": role,
        })),
    )
        .into_response()
}

async fn logout(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    // Extract session from cookie
    if let Some(cookie_header) = headers.get("cookie") {
        if let Ok(cookies) = cookie_header.to_str() {
            for pair in cookies.split(';') {
                let pair = pair.trim();
                if let Some(session_id) = pair.strip_prefix("claw_session=") {
                    let _ = claw_redis::delete_session(&state.pool, session_id.trim()).await;
                }
            }
        }
    }

    // Clear cookie
    let secure_flag = if std::env::var("CLAW_CORS_ORIGIN")
        .unwrap_or_default()
        .starts_with("https")
    {
        "; Secure"
    } else {
        ""
    };
    let clear = format!(
        "claw_session=; HttpOnly; SameSite=Lax; Path=/; Max-Age=0{}",
        secure_flag
    );
    let mut resp_headers = HeaderMap::new();
    resp_headers.insert("set-cookie", clear.parse().unwrap());

    (StatusCode::OK, resp_headers, Json(serde_json::json!({"ok": true}))).into_response()
}

async fn me(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    // Extract session from cookie
    let session_id = headers
        .get("cookie")
        .and_then(|v| v.to_str().ok())
        .and_then(|cookies| {
            cookies.split(';').find_map(|pair| {
                let pair = pair.trim();
                pair.strip_prefix("claw_session=")
                    .map(|v| v.trim().to_string())
            })
        });

    let Some(session_id) = session_id else {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "Not authenticated"})),
        )
            .into_response();
    };

    let username = match claw_redis::get_session(&state.pool, &session_id).await {
        Ok(Some(u)) => u,
        _ => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({"error": "Session expired"})),
            )
                .into_response();
        }
    };

    let role = match claw_redis::get_user(&state.pool, &username).await {
        Ok(Some(u)) => u.get("role").cloned().unwrap_or_else(|| "user".to_string()),
        _ => "user".to_string(),
    };

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "username": username,
            "role": role,
        })),
    )
        .into_response()
}

#[derive(Deserialize)]
struct CreateUserRequest {
    username: String,
    password: String,
    #[serde(default = "default_role")]
    role: String,
}

fn default_role() -> String {
    "user".to_string()
}

async fn create_user(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<CreateUserRequest>,
) -> impl IntoResponse {
    // Check caller is admin
    if let Err(resp) = require_admin(&state, &headers).await {
        return resp;
    }

    if req.username.is_empty() || req.password.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "Username and password are required"})),
        )
            .into_response();
    }

    let hash = match bcrypt::hash(&req.password, bcrypt::DEFAULT_COST) {
        Ok(h) => h,
        Err(e) => {
            tracing::error!(error = %e, "Failed to hash password");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "Internal error"})),
            )
                .into_response();
        }
    };

    match claw_redis::create_user(&state.pool, &req.username, &hash, &req.role).await {
        Ok(()) => (
            StatusCode::CREATED,
            Json(serde_json::json!({
                "username": req.username,
                "role": req.role,
            })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::CONFLICT,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

async fn list_users(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(resp) = require_admin(&state, &headers).await {
        return resp;
    }

    match claw_redis::list_users(&state.pool).await {
        Ok(users) => {
            let items: Vec<_> = users
                .into_iter()
                .map(|(username, role)| serde_json::json!({"username": username, "role": role}))
                .collect();
            (StatusCode::OK, Json(serde_json::json!({"items": items}))).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

async fn delete_user(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path(username): axum::extract::Path<String>,
) -> impl IntoResponse {
    if let Err(resp) = require_admin(&state, &headers).await {
        return resp;
    }

    match claw_redis::delete_user(&state.pool, &username).await {
        Ok(()) => (StatusCode::OK, Json(serde_json::json!({"ok": true}))).into_response(),
        Err(e) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

/// Helper: require the calling user to have admin role.
async fn require_admin(state: &AppState, headers: &HeaderMap) -> Result<(), Response> {
    // Valid bearer token holders are implicitly admin
    if let Some(auth_header) = headers.get("authorization") {
        if let Ok(value) = auth_header.to_str() {
            if let Some(token) = value.strip_prefix("Bearer ") {
                // Must match the configured API token
                if let Ok(expected) = std::env::var("CLAW_API_TOKEN") {
                    if !expected.is_empty() && token == expected {
                        return Ok(());
                    }
                }
            }
        }
    }

    // Session-based: look up user role
    let session_id = headers
        .get("cookie")
        .and_then(|v| v.to_str().ok())
        .and_then(|cookies| {
            cookies.split(';').find_map(|pair| {
                let pair = pair.trim();
                pair.strip_prefix("claw_session=")
                    .map(|v| v.trim().to_string())
            })
        });

    let Some(session_id) = session_id else {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "Authentication required"})),
        )
            .into_response());
    };

    let username = match claw_redis::get_session(&state.pool, &session_id).await {
        Ok(Some(u)) => u,
        _ => {
            return Err((
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({"error": "Session expired"})),
            )
                .into_response());
        }
    };

    let user = match claw_redis::get_user(&state.pool, &username).await {
        Ok(Some(u)) => u,
        _ => {
            return Err((
                StatusCode::FORBIDDEN,
                Json(serde_json::json!({"error": "User not found"})),
            )
                .into_response());
        }
    };

    let role = user.get("role").cloned().unwrap_or_default();
    if role != "admin" {
        return Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "Admin access required"})),
        )
            .into_response());
    }

    Ok(())
}
