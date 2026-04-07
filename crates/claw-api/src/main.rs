mod auth;
mod routes;
pub mod upload_utils;
pub mod util;

use axum::Router;
use axum::http::{header, Method};
use deadpool_redis::Pool;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::services::{ServeDir, ServeFile};

#[derive(Clone)]
pub struct AppState {
    pub pool: Pool,
    pub redis_url: String,
}

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| "info,claw_api=debug".into());
    if std::env::var("CLAW_LOG_FORMAT").as_deref() == Ok("json") {
        tracing_subscriber::fmt().json().with_env_filter(filter).init();
    } else {
        tracing_subscriber::fmt().with_env_filter(filter).init();
    }

    let redis_url = std::env::var("CLAW_REDIS_URL")
        .unwrap_or_else(|_| "redis://127.0.0.1:6379".into());
    let port = std::env::var("CLAW_API_PORT").unwrap_or_else(|_| "8080".into());

    let pool = claw_redis::create_pool(&redis_url);
    let state = AppState { pool, redis_url };

    // Bootstrap admin user if none exist
    auth::bootstrap_admin(&state.pool).await;

    // Ensure workspace directories exist
    let home = dirs::home_dir().unwrap_or_else(|| "/tmp".into());
    for subdir in &["repos", "checkouts"] {
        let dir = home.join(".claw").join(subdir);
        if let Err(e) = tokio::fs::create_dir_all(&dir).await {
            tracing::warn!(dir = %dir.display(), error = %e, "Failed to create directory");
        } else {
            tracing::info!(dir = %dir.display(), "Ensured directory exists");
        }
    }

    let static_dir = std::env::var("CLAW_STATIC_DIR")
        .unwrap_or_else(|_| "flutter_ui/build/web".into());
    tracing::info!(static_dir, "Serving static files");

    // Build CORS layer
    let cors = build_cors_layer();

    // Clone pool for background task before state is moved
    let sync_pool = state.pool.clone();

    // Build API routes with state first
    let api = Router::new()
        .nest("/api/v1", routes::router())
        .with_state(state.clone())
        .layer(axum::middleware::from_fn_with_state(state, auth::auth_middleware));

    // Combine API routes with static file fallback
    let app = api
        .fallback_service(
            ServeDir::new(&static_dir)
                .fallback(ServeFile::new(format!("{static_dir}/index.html"))),
        )
        .layer(cors);

    // Background catalog sync (10s after startup)
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(10)).await;
        match routes::catalog::do_sync(&sync_pool).await {
            Ok(summary) => tracing::info!(?summary, "Catalog auto-sync complete"),
            Err(e) => tracing::warn!(error = %e, "Catalog auto-sync failed (non-fatal)"),
        }
    });

    let addr = format!("0.0.0.0:{port}");
    tracing::info!("claw-api listening on {addr}");

    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

/// Build CORS layer. If CLAW_CORS_ORIGIN is set, restrict to that origin.
/// Otherwise fall back to permissive (development mode).
fn build_cors_layer() -> CorsLayer {
    if let Ok(origin) = std::env::var("CLAW_CORS_ORIGIN") {
        if !origin.is_empty() {
            tracing::info!(origin, "CORS restricted to configured origin");
            return CorsLayer::new()
                .allow_origin(AllowOrigin::exact(origin.parse().unwrap()))
                .allow_methods([
                    Method::GET,
                    Method::POST,
                    Method::PUT,
                    Method::DELETE,
                    Method::OPTIONS,
                ])
                .allow_headers([
                    header::CONTENT_TYPE,
                    header::AUTHORIZATION,
                    header::ACCEPT,
                    header::COOKIE,
                ])
                .allow_credentials(true);
        }
    }
    tracing::warn!("CLAW_CORS_ORIGIN not set — using permissive CORS (development mode)");
    CorsLayer::permissive()
}
