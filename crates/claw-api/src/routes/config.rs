use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use std::collections::HashMap;

use crate::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/config", get(get_all).put(update_bulk))
        .route("/config/{key}", get(get_one).put(set_one))
}

/// Config keys whose values must be redacted in GET responses.
const SENSITIVE_CONFIG_KEYS: &[&str] = &["anthropic_api_key"];

fn redact_sensitive(mut config: HashMap<String, String>) -> HashMap<String, String> {
    for key in SENSITIVE_CONFIG_KEYS {
        if let Some(val) = config.get_mut(*key) {
            if !val.is_empty() {
                *val = "***set***".to_string();
            }
        }
    }
    config
}

async fn get_all(State(state): State<AppState>) -> impl IntoResponse {
    match claw_redis::get_all_config(&state.pool).await {
        Ok(config) => Json(redact_sensitive(config)).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

async fn get_one(
    State(state): State<AppState>,
    Path(key): Path<String>,
) -> impl IntoResponse {
    match claw_redis::get_config(&state.pool, &key).await {
        Ok(val) => {
            let display_val = if SENSITIVE_CONFIG_KEYS.contains(&key.as_str()) && !val.is_empty() {
                "***set***".to_string()
            } else {
                val
            };
            Json(serde_json::json!({"key": key, "value": display_val})).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

async fn update_bulk(
    State(state): State<AppState>,
    Json(values): Json<HashMap<String, String>>,
) -> impl IntoResponse {
    match claw_redis::set_configs(&state.pool, &values).await {
        Ok(()) => Json(serde_json::json!({"updated": values.len()})).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

#[derive(serde::Deserialize)]
struct SetValueRequest {
    value: String,
}

async fn set_one(
    State(state): State<AppState>,
    Path(key): Path<String>,
    Json(req): Json<SetValueRequest>,
) -> impl IntoResponse {
    match claw_redis::set_config(&state.pool, &key, &req.value).await {
        Ok(()) => {
            let display_val = if SENSITIVE_CONFIG_KEYS.contains(&key.as_str()) && !req.value.is_empty() {
                "***set***".to_string()
            } else {
                req.value
            };
            Json(serde_json::json!({"key": key, "value": display_val})).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}
