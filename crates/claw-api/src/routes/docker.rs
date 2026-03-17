use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use tokio::process::Command;

use crate::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/docker/status", get(docker_status))
        .route("/docker/images", get(list_images))
        .route("/docker/images/pull", post(pull_image))
        .route("/docker/images/build", post(build_image))
}

/// Check if Docker is available and return basic info.
async fn docker_status(_state: State<AppState>) -> impl IntoResponse {
    match Command::new("docker").arg("info").arg("--format").arg("{{json .}}").output().await {
        Ok(output) if output.status.success() => {
            let info_str = String::from_utf8_lossy(&output.stdout);
            let info: serde_json::Value =
                serde_json::from_str(&info_str).unwrap_or(serde_json::json!({}));
            Json(serde_json::json!({
                "available": true,
                "server_version": info.get("ServerVersion").and_then(|v| v.as_str()).unwrap_or("unknown"),
                "os": info.get("OperatingSystem").and_then(|v| v.as_str()).unwrap_or("unknown"),
                "containers_running": info.get("ContainersRunning").and_then(|v| v.as_u64()).unwrap_or(0),
            }))
            .into_response()
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Json(serde_json::json!({
                "available": false,
                "error": stderr.trim(),
            }))
            .into_response()
        }
        Err(e) => Json(serde_json::json!({
            "available": false,
            "error": format!("docker not found: {e}"),
        }))
        .into_response(),
    }
}

/// List Docker images matching the sandbox image name.
async fn list_images(State(state): State<AppState>) -> impl IntoResponse {
    let sandbox_image = claw_redis::get_config(&state.pool, "sandbox_image")
        .await
        .unwrap_or_else(|_| "claw-sandbox".to_string());

    // List images with name filter
    let output = Command::new("docker")
        .args(["images", "--format", "{{json .}}", "--filter", &format!("reference={}*", sandbox_image)])
        .output()
        .await;

    match output {
        Ok(out) if out.status.success() => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            let images: Vec<serde_json::Value> = stdout
                .lines()
                .filter_map(|line| serde_json::from_str(line).ok())
                .collect();
            Json(serde_json::json!({"images": images})).into_response()
        }
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": stderr.trim()})),
            )
                .into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("docker not found: {e}")})),
        )
            .into_response(),
    }
}

#[derive(serde::Deserialize)]
struct PullRequest {
    image: Option<String>,
}

/// Validate a Docker image name/tag (basic sanity check).
fn is_valid_docker_ref(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 256
        && !s.contains(';')
        && !s.contains('&')
        && !s.contains('|')
        && !s.contains('$')
        && !s.contains('`')
        && !s.contains('\n')
}

/// Pull a Docker image. Uses configured sandbox_image if no image specified.
async fn pull_image(
    State(state): State<AppState>,
    Json(req): Json<PullRequest>,
) -> impl IntoResponse {
    let image = match req.image {
        Some(img) => img,
        None => claw_redis::get_config(&state.pool, "sandbox_image")
            .await
            .unwrap_or_else(|_| "claw-sandbox:latest".to_string()),
    };

    if !is_valid_docker_ref(&image) {
        return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": "Invalid image name"}))).into_response();
    }

    let output = Command::new("docker")
        .args(["pull", &image])
        .output()
        .await;

    match output {
        Ok(out) if out.status.success() => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            Json(serde_json::json!({
                "success": true,
                "image": image,
                "output": stdout.trim(),
            }))
            .into_response()
        }
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(serde_json::json!({
                    "success": false,
                    "image": image,
                    "error": stderr.trim(),
                })),
            )
                .into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("docker not found: {e}")})),
        )
            .into_response(),
    }
}

#[derive(serde::Deserialize)]
struct BuildRequest {
    tag: Option<String>,
}

/// Build the sandbox Docker image from the bundled Dockerfile.
async fn build_image(
    State(state): State<AppState>,
    Json(req): Json<BuildRequest>,
) -> impl IntoResponse {
    let tag = match req.tag {
        Some(t) => t,
        None => claw_redis::get_config(&state.pool, "sandbox_image")
            .await
            .unwrap_or_else(|_| "claw-sandbox:latest".to_string()),
    };

    if !is_valid_docker_ref(&tag) {
        return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": "Invalid image tag"}))).into_response();
    }

    // Look for Dockerfile.sandbox in known locations
    let dockerfile_paths = [
        "/app/docker/Dockerfile.sandbox",      // inside API container
        "docker/Dockerfile.sandbox",           // repo root (dev mode)
    ];

    let dockerfile = match dockerfile_paths.iter().find(|p| std::path::Path::new(p).exists()) {
        Some(p) => *p,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({
                    "error": "Dockerfile.sandbox not found. Looked in: docker/Dockerfile.sandbox, /app/docker/Dockerfile.sandbox"
                })),
            )
                .into_response();
        }
    };

    // Determine build context (directory containing the Dockerfile)
    let context = std::path::Path::new(dockerfile)
        .parent()
        .unwrap_or(std::path::Path::new("."));

    let output = Command::new("docker")
        .args(["build", "-f", dockerfile, "-t", &tag, &context.to_string_lossy()])
        .output()
        .await;

    match output {
        Ok(out) if out.status.success() => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            Json(serde_json::json!({
                "success": true,
                "tag": tag,
                "output": stdout.chars().take(2000).collect::<String>(),
            }))
            .into_response()
        }
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "success": false,
                    "tag": tag,
                    "error": stderr.chars().take(2000).collect::<String>(),
                })),
            )
                .into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("docker not found: {e}")})),
        )
            .into_response(),
    }
}
