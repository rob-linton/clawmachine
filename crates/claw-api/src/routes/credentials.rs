use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};

use crate::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/credentials", post(create_credential).get(list_credentials))
        .route(
            "/credentials/{id}",
            get(get_credential).put(update_credential).delete(delete_credential),
        )
}

async fn create_credential(
    State(state): State<AppState>,
    Json(req): Json<claw_models::CreateCredentialRequest>,
) -> impl IntoResponse {
    match claw_redis::create_credential(&state.pool, &req).await {
        Ok(cred) => (StatusCode::CREATED, Json(serde_json::json!(cred))).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

async fn list_credentials(State(state): State<AppState>) -> impl IntoResponse {
    match claw_redis::list_credentials(&state.pool).await {
        Ok(creds) => {
            Json(serde_json::json!({"items": creds, "total": creds.len()})).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

async fn get_credential(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match claw_redis::get_credential(&state.pool, &id).await {
        Ok(Some(cred)) => {
            // Return metadata only, mask values
            let masked: std::collections::HashMap<String, String> = cred
                .keys
                .iter()
                .map(|k| (k.clone(), "***set***".to_string()))
                .collect();
            let resp = claw_models::CredentialResponse {
                id: cred.id,
                name: cred.name,
                description: cred.description,
                keys: cred.keys,
                masked_values: masked,
                created_at: cred.created_at,
                updated_at: cred.updated_at,
            };
            Json(resp).into_response()
        }
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

async fn update_credential(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<claw_models::CreateCredentialRequest>,
) -> impl IntoResponse {
    match claw_redis::update_credential(&state.pool, &id, &req).await {
        Ok(cred) => Json(serde_json::json!(cred)).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

async fn delete_credential(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match claw_redis::delete_credential(&state.pool, &id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}
