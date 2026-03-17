use axum::{
    extract::Request,
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
};
use std::sync::OnceLock;

static API_TOKEN: OnceLock<Option<String>> = OnceLock::new();

fn get_api_token() -> &'static Option<String> {
    API_TOKEN.get_or_init(|| {
        let token = std::env::var("CLAW_API_TOKEN").ok().filter(|t| !t.is_empty());
        if token.is_some() {
            tracing::info!("API authentication enabled (CLAW_API_TOKEN is set)");
        } else {
            tracing::warn!("API authentication disabled (CLAW_API_TOKEN not set) — all endpoints are publicly accessible");
        }
        token
    })
}

pub async fn auth_middleware(req: Request, next: Next) -> Response {
    let Some(expected_token) = get_api_token() else {
        // No token configured — allow all requests (development mode)
        return next.run(req).await;
    };

    // Check Authorization header: "Bearer <token>"
    if let Some(auth_header) = req.headers().get("authorization") {
        if let Ok(value) = auth_header.to_str() {
            if let Some(token) = value.strip_prefix("Bearer ") {
                if token == expected_token {
                    return next.run(req).await;
                }
            }
        }
    }

    // Check query parameter: ?token=<token>
    if let Some(query) = req.uri().query() {
        for pair in query.split('&') {
            if let Some(token) = pair.strip_prefix("token=") {
                if token == expected_token {
                    return next.run(req).await;
                }
            }
        }
    }

    (
        StatusCode::UNAUTHORIZED,
        axum::Json(serde_json::json!({"error": "Missing or invalid API token. Set Authorization: Bearer <token> header."})),
    )
        .into_response()
}
