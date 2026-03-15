mod routes;

use axum::Router;
use deadpool_redis::Pool;
use tower_http::cors::CorsLayer;
use tower_http::services::{ServeDir, ServeFile};

#[derive(Clone)]
pub struct AppState {
    pub pool: Pool,
}

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,claw_api=debug".into()),
        )
        .init();

    let redis_url = std::env::var("CLAW_REDIS_URL")
        .unwrap_or_else(|_| "redis://127.0.0.1:6379".into());
    let port = std::env::var("CLAW_API_PORT").unwrap_or_else(|_| "8080".into());

    let pool = claw_redis::create_pool(&redis_url);
    let state = AppState { pool };

    let static_dir = std::env::var("CLAW_STATIC_DIR")
        .unwrap_or_else(|_| "flutter_ui/build/web".into());
    tracing::info!(static_dir, "Serving static files");

    // Build API routes with state first
    let api = Router::new()
        .nest("/api/v1", routes::router())
        .with_state(state);

    // Combine API routes with static file fallback
    let app = api
        .fallback_service(
            ServeDir::new(&static_dir)
                .fallback(ServeFile::new(format!("{static_dir}/index.html"))),
        )
        .layer(CorsLayer::permissive());

    let addr = format!("0.0.0.0:{port}");
    tracing::info!("claw-api listening on {addr}");

    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
