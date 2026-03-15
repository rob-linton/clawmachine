use deadpool_redis::Pool;
use notify::{Event, EventKind, RecursiveMode, Watcher};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use claw_models::*;

/// Watch a directory for .job files and submit them as jobs.
pub async fn run(pool: Pool, dir: &str, shutdown: Arc<AtomicBool>) {
    let path = Path::new(dir);
    if !path.exists() {
        tracing::warn!(dir, "Watch directory doesn't exist, creating it");
        std::fs::create_dir_all(path).ok();
    }

    tracing::info!(dir, "File watcher started");

    // Use a std sync channel between the notify callback and our async loop
    let (tx, rx) = std::sync::mpsc::channel::<String>();

    let mut watcher = notify::recommended_watcher(move |res: Result<Event, _>| {
        if let Ok(event) = res {
            if matches!(event.kind, EventKind::Create(_) | EventKind::Modify(_)) {
                for path in &event.paths {
                    if let Some(ext) = path.extension() {
                        if ext == "job" {
                            tx.send(path.to_string_lossy().to_string()).ok();
                        }
                    }
                }
            }
        }
    })
    .expect("Failed to create file watcher");

    watcher
        .watch(Path::new(dir), RecursiveMode::NonRecursive)
        .expect("Failed to watch directory");

    // Also scan for existing .job files on startup
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "job") {
                process_job_file(&pool, &path.to_string_lossy()).await;
            }
        }
    }

    // Process events from the watcher
    loop {
        if shutdown.load(Ordering::Relaxed) {
            break;
        }

        // Non-blocking check with timeout
        match rx.recv_timeout(std::time::Duration::from_secs(2)) {
            Ok(file_path) => {
                // Small delay to let the file finish writing
                tokio::time::sleep(std::time::Duration::from_millis(300)).await;
                process_job_file(&pool, &file_path).await;
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }

    // Drop watcher to stop watching (it's moved into scope here)
    drop(watcher);
}

async fn process_job_file(pool: &Pool, file_path: &str) {
    let path = Path::new(file_path);
    if !path.exists() || file_path.ends_with(".tmp") {
        return;
    }

    tracing::info!(file = %file_path, "Processing .job file");

    match std::fs::read_to_string(path) {
        Ok(content) => match serde_json::from_str::<CreateJobRequest>(&content) {
            Ok(req) => match claw_redis::submit_job(pool, &req, JobSource::FileWatcher).await {
                Ok(job) => {
                    tracing::info!(job_id = %job.id, file = %file_path, "Job submitted from .job file");
                    let submitted = format!("{}.submitted", file_path);
                    std::fs::rename(path, &submitted).ok();
                }
                Err(e) => tracing::error!(file = %file_path, error = %e, "Failed to submit"),
            },
            Err(e) => {
                tracing::error!(file = %file_path, error = %e, "Failed to parse .job file");
                std::fs::rename(path, format!("{}.error", file_path)).ok();
            }
        },
        Err(e) => tracing::error!(file = %file_path, error = %e, "Failed to read"),
    }
}
