use claw_models::Job;
use std::path::Path;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::executor::ExecutionResult;

/// Configuration for Docker-based execution.
pub struct DockerConfig {
    pub image: String,
    pub memory_limit: String,
    pub cpu_limit: String,
    pub pids_limit: String,
    pub credential_mounts: Vec<CredentialMount>,
}

#[derive(Clone, serde::Deserialize)]
pub struct CredentialMount {
    pub host_path: String,
    pub container_path: String,
    #[serde(default = "default_true")]
    pub readonly: bool,
}

fn default_true() -> bool {
    true
}

impl DockerConfig {
    /// Load config from Redis values.
    pub fn from_config(config: &std::collections::HashMap<String, String>) -> Self {
        let mounts: Vec<CredentialMount> = config
            .get("docker_credential_mounts")
            .and_then(|s| serde_json::from_str(s).ok())
            .unwrap_or_default();

        Self {
            image: config
                .get("sandbox_image")
                .cloned()
                .unwrap_or_else(|| "claw-sandbox:latest".into()),
            memory_limit: config
                .get("docker_memory_limit")
                .cloned()
                .unwrap_or_else(|| "4g".into()),
            cpu_limit: config
                .get("docker_cpu_limit")
                .cloned()
                .unwrap_or_else(|| "2.0".into()),
            pids_limit: config
                .get("docker_pids_limit")
                .cloned()
                .unwrap_or_else(|| "256".into()),
            credential_mounts: mounts,
        }
    }
}

/// Ensure the sandbox image exists locally. Pull if missing, build if pull fails.
pub async fn ensure_image(image: &str) -> Result<(), String> {
    // Check if image exists
    let check = Command::new("docker")
        .args(["image", "inspect", image])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await;

    if let Ok(status) = check {
        if status.success() {
            tracing::info!(image, "Sandbox image already available");
            return Ok(());
        }
    }

    // Try to pull
    tracing::info!(image, "Sandbox image not found, attempting pull...");
    let pull = Command::new("docker")
        .args(["pull", image])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .status()
        .await;

    if let Ok(status) = pull {
        if status.success() {
            tracing::info!(image, "Sandbox image pulled successfully");
            return Ok(());
        }
    }

    // Try to build from known Dockerfile locations
    tracing::info!(image, "Pull failed, attempting local build...");
    let dockerfile_paths = [
        "docker/Dockerfile.sandbox",
        "/app/docker/Dockerfile.sandbox",
    ];

    for path in &dockerfile_paths {
        if Path::new(path).exists() {
            let context = Path::new(path).parent().unwrap_or(Path::new("."));
            let build = Command::new("docker")
                .args(["build", "-f", path, "-t", image, &context.to_string_lossy()])
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .status()
                .await;

            if let Ok(status) = build {
                if status.success() {
                    tracing::info!(image, dockerfile = path, "Sandbox image built successfully");
                    return Ok(());
                }
            }
        }
    }

    Err(format!(
        "Could not ensure sandbox image '{image}': not found, pull failed, and no Dockerfile.sandbox available"
    ))
}

/// Execute a job inside a Docker container.
pub async fn docker_execute_job(
    job: &Job,
    working_dir: &Path,
    config: &DockerConfig,
    system_prompt: Option<&str>,
    log_tx: mpsc::Sender<String>,
    cancel: CancellationToken,
) -> Result<ExecutionResult, String> {
    let timeout_secs = job.timeout_secs.unwrap_or(3600);
    let timeout = std::time::Duration::from_secs(timeout_secs);
    let prompt = job.assembled_prompt.as_deref().unwrap_or(&job.prompt);

    // Resolve workspace-specific image override
    let image = &config.image;

    // Build docker run command
    let container_name = format!("claw-job-{}", job.id);
    let uid_gid = format!("{}:{}", users::get_current_uid(), users::get_current_gid());

    let mut args: Vec<String> = vec![
        "run".into(),
        "-d".into(), // detached — we stream logs separately
        "--name".into(),
        container_name.clone(),
        "--user".into(),
        uid_gid,
        "--workdir".into(),
        "/workspace".into(),
        // Resource limits
        "--memory".into(),
        config.memory_limit.clone(),
        "--cpus".into(),
        config.cpu_limit.clone(),
        "--pids-limit".into(),
        config.pids_limit.clone(),
        // Mount workspace
        "-v".into(),
        format!("{}:/workspace", working_dir.to_string_lossy()),
    ];

    // Mount credentials
    for mount in &config.credential_mounts {
        let host = expand_tilde(&mount.host_path);
        if !Path::new(&host).exists() {
            continue; // Skip non-existent credential paths
        }
        let mode = if mount.readonly { "ro" } else { "rw" };
        args.push("-v".into());
        args.push(format!("{}:{}:{}", host, mount.container_path, mode));
    }

    // Image
    args.push(image.clone());

    // Claude arguments
    args.push("-p".into());
    args.push(prompt.to_string());
    args.push("--output-format".into());
    args.push("stream-json".into());
    args.push("--verbose".into()); // required by --output-format stream-json
    args.push("--dangerously-skip-permissions".into());

    if let Some(model) = &job.model {
        args.push("--model".into());
        args.push(model.clone());
    }

    if let Some(budget) = job.max_budget_usd {
        args.push("--max-budget-usd".into());
        args.push(budget.to_string());
    }

    // Only restrict tools when the job explicitly specifies a tool list
    if let Some(tools) = &job.allowed_tools {
        if !tools.is_empty() {
            args.push("--allowedTools".into());
            args.push(tools.join(","));
        }
    }

    // Append system prompt with metadata + completion instruction
    if let Some(sp) = system_prompt {
        args.push("--append-system-prompt".into());
        args.push(sp.to_string());
    }

    let start = std::time::Instant::now();

    // Start container
    let start_output = Command::new("docker")
        .args(&args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .await
        .map_err(|e| format!("Failed to start docker container: {e}"))?;

    if !start_output.status.success() {
        let stderr = String::from_utf8_lossy(&start_output.stderr);
        // Best-effort cleanup in case container was partially created
        cleanup_container(&container_name).await;
        return Err(format!("docker run failed: {}", stderr.trim()));
    }

    let container_id = String::from_utf8_lossy(&start_output.stdout)
        .trim()
        .to_string();

    tracing::info!(container = %container_id, job_id = %job.id, "Docker container started");

    // Stream logs
    let mut log_cmd = Command::new("docker");
    log_cmd.args(["logs", "-f", &container_id]);
    log_cmd.stdout(std::process::Stdio::piped());
    log_cmd.stderr(std::process::Stdio::piped());
    log_cmd.kill_on_drop(true);

    let mut log_child = log_cmd
        .spawn()
        .map_err(|e| format!("Failed to stream docker logs: {e}"))?;

    let stdout = log_child.stdout.take().ok_or("No log stdout")?;
    let reader = BufReader::new(stdout);
    let mut lines = reader.lines();
    let mut state = crate::executor::StreamState::new();

    let container_for_cancel = container_id.clone();
    let container_for_timeout = container_id.clone();

    let output = tokio::select! {
        r = async {
            while let Ok(Some(line)) = lines.next_line().await {
                let _ = log_tx.try_send(line.clone());
                state.process_line(&line);
            }
            // Wait for container to actually exit
            Command::new("docker")
                .args(["wait", &container_id])
                .output()
                .await
        } => r,

        _ = cancel.cancelled() => {
            Command::new("docker").args(["kill", &container_for_cancel])
                .output().await.ok();
            cleanup_container(&container_for_cancel).await;
            return Err("Job was cancelled".to_string());
        }

        _ = tokio::time::sleep(timeout) => {
            Command::new("docker").args(["stop", "-t", "5", &container_for_timeout])
                .output().await.ok();
            cleanup_container(&container_for_timeout).await;
            return Err(format!("Job timed out after {}s", timeout_secs));
        }
    };

    let duration_ms = start.elapsed().as_millis() as u64;

    // Check exit code
    let exit_code = match output {
        Ok(o) if o.status.success() => {
            String::from_utf8_lossy(&o.stdout)
                .trim()
                .parse::<i32>()
                .unwrap_or(-1)
        }
        _ => -1,
    };

    // Clean up container
    cleanup_container(&container_id).await;

    if exit_code != 0 {
        return Err(format!("claude exited with code {exit_code} inside container"));
    }

    let (result_text, cost_usd) = state.finalize();

    Ok(ExecutionResult {
        result_text,
        cost_usd,
        duration_ms,
    })
}

async fn cleanup_container(container_id: &str) {
    Command::new("docker")
        .args(["rm", "-f", container_id])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await
        .ok();
}

fn expand_tilde(path: &str) -> String {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest).to_string_lossy().to_string();
        }
    }
    path.to_string()
}
