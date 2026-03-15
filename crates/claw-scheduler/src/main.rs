mod cron_engine;
mod watcher;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,claw_scheduler=debug".into()),
        )
        .init();

    let redis_url = std::env::var("CLAW_REDIS_URL")
        .unwrap_or_else(|_| "redis://127.0.0.1:6379".into());
    let watch_dir = std::env::var("CLAW_JOBS_WATCH_DIR")
        .unwrap_or_else(|_| "jobs".into());
    let cron_interval: u64 = std::env::var("CLAW_CRON_CHECK_INTERVAL_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(30);

    let pool = claw_redis::create_pool(&redis_url);
    let shutdown = Arc::new(AtomicBool::new(false));

    // Test connection
    {
        let mut conn = pool.get().await.expect("Failed to connect to Redis");
        let _: String = deadpool_redis::redis::cmd("PING")
            .query_async(&mut *conn)
            .await
            .expect("Redis PING failed");
    }

    tracing::info!(watch_dir, cron_interval, "claw-scheduler starting");

    let mut handles = Vec::new();

    // Cron engine
    {
        let pool = pool.clone();
        let shutdown = shutdown.clone();
        handles.push(tokio::spawn(async move {
            cron_engine::run(pool, cron_interval, shutdown).await;
        }));
    }

    // File watcher
    {
        let pool = pool.clone();
        let shutdown = shutdown.clone();
        let dir = watch_dir.clone();
        handles.push(tokio::spawn(async move {
            watcher::run(pool, &dir, shutdown).await;
        }));
    }

    tokio::signal::ctrl_c().await.ok();
    tracing::info!("Shutting down...");
    shutdown.store(true, Ordering::Relaxed);

    for h in handles {
        h.await.ok();
    }
    tracing::info!("Scheduler stopped");
}
