use claw_models::{Job, Workspace};
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

/// Per-job image availability check with 60s cache.
/// Avoids running `docker image inspect` on every single job.
static IMAGE_CHECK_CACHE: std::sync::OnceLock<tokio::sync::Mutex<(String, std::time::Instant)>> =
    std::sync::OnceLock::new();

pub async fn check_image_cached(image: &str) -> Result<(), String> {
    let cache = IMAGE_CHECK_CACHE.get_or_init(|| {
        tokio::sync::Mutex::new((String::new(), std::time::Instant::now() - std::time::Duration::from_secs(120)))
    });
    let mut guard = cache.lock().await;
    if guard.0 == image && guard.1.elapsed().as_secs() < 60 {
        return Ok(()); // Cache hit
    }
    // Cache miss — do a quick check
    let check = Command::new("docker")
        .args(["image", "inspect", image])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await;
    match check {
        Ok(status) if status.success() => {
            *guard = (image.to_string(), std::time::Instant::now());
            Ok(())
        }
        _ => Err(format!("Sandbox image '{}' not available. Build with: POST /api/v1/docker/images/build", image)),
    }
}

/// Check that the Docker socket is accessible.
pub async fn check_docker_socket() -> Result<(), String> {
    let output = Command::new("docker")
        .args(["info", "--format", "{{.ServerVersion}}"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .await
        .map_err(|e| format!("Failed to run docker info: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "Docker socket not accessible. Ensure /var/run/docker.sock is mounted: {}",
            stderr.trim()
        ));
    }
    Ok(())
}

/// Translate a container-local path to a host path for Docker volume mounts.
/// When the worker runs inside a container, paths like /home/claw/.claw/jobs/{id}
/// must be translated to the host equivalent (e.g., /opt/claw/data/jobs/{id}).
fn translate_to_host_path(container_path: &Path) -> String {
    let container_str = container_path.to_string_lossy();

    // If CLAW_HOST_DATA_DIR is set, translate ~/.claw → host path
    if let Ok(host_data_dir) = std::env::var("CLAW_HOST_DATA_DIR") {
        let home = dirs::home_dir().unwrap_or_else(|| "/home/claw".into());
        let container_claw_dir = home.join(".claw").to_string_lossy().to_string();
        if container_str.starts_with(&container_claw_dir) {
            return container_str.replace(&container_claw_dir, &host_data_dir);
        }
    }

    // No translation needed (local mode or path not under ~/.claw)
    container_str.to_string()
}

/// Translate credential mount paths to host paths for Docker-in-Docker.
fn translate_credential_host_path(host_path: &str) -> String {
    // CLAW_HOST_CLAUDE_HOME overrides ~/.claude paths
    if let Ok(host_claude) = std::env::var("CLAW_HOST_CLAUDE_HOME") {
        let home = dirs::home_dir().unwrap_or_else(|| "/home/claw".into());
        let local_claude = home.join(".claude").to_string_lossy().to_string();
        let expanded = expand_tilde(host_path);
        if expanded == local_claude || expanded.starts_with(&format!("{}/", local_claude)) {
            return expanded.replace(&local_claude, &host_claude);
        }
    }
    expand_tilde(host_path)
}

/// Execute a job inside a Docker container.
pub async fn docker_execute_job(
    job: &Job,
    working_dir: &Path,
    config: &DockerConfig,
    workspace: Option<&Workspace>,
    system_prompt: Option<&str>,
    log_tx: mpsc::Sender<String>,
    cancel: CancellationToken,
) -> Result<ExecutionResult, String> {
    let timeout_secs = job.timeout_secs.unwrap_or(3600);
    let timeout = std::time::Duration::from_secs(timeout_secs);
    let prompt = job.assembled_prompt.as_deref().unwrap_or(&job.prompt);

    // Merge: workspace overrides > global config > defaults
    let image = workspace
        .and_then(|w| w.base_image.as_deref())
        .unwrap_or(&config.image);
    let memory = workspace
        .and_then(|w| w.memory_limit.as_deref())
        .unwrap_or(&config.memory_limit);
    let cpu = workspace
        .and_then(|w| w.cpu_limit.map(|c| c.to_string()))
        .unwrap_or_else(|| config.cpu_limit.clone());
    let network = workspace
        .and_then(|w| w.network_mode.as_deref())
        .unwrap_or("bridge"); // Claude Code requires network for Anthropic API

    // Build docker run command
    let container_name = format!("claw-job-{}", job.id);
    // Use UID/GID of the workspace files owner, not the worker process.
    // Worker runs as root (for Docker socket), but Claude Code refuses
    // --dangerously-skip-permissions as root. Use the claw user (1000) or
    // the actual owner of the workspace directory.
    let (uid, gid) = {
        let current_uid = users::get_current_uid();
        if current_uid == 0 {
            // Running as root — find the owner of the workspace dir
            std::fs::metadata(working_dir)
                .map(|m| {
                    use std::os::unix::fs::MetadataExt;
                    (m.uid(), m.gid())
                })
                .unwrap_or((1000, 1000))
        } else {
            (current_uid, users::get_current_gid())
        }
    };
    let uid_gid = format!("{}:{}", uid, gid);

    // Translate workspace path to host path for Docker volume mount
    let host_workspace_path = translate_to_host_path(working_dir);

    let mut args: Vec<String> = vec![
        "run".into(),
        "-d".into(), // detached — we stream logs separately
        "--name".into(),
        container_name.clone(),
        "--user".into(),
        uid_gid,
        "--workdir".into(),
        "/workspace".into(),
        // Network mode
        "--network".into(),
        network.to_string(),
        // Resource limits
        "--memory".into(),
        memory.to_string(),
        "--cpus".into(),
        cpu,
        "--pids-limit".into(),
        config.pids_limit.clone(),
        // Mount workspace (using host path)
        "-v".into(),
        format!("{}:/workspace", host_workspace_path),
    ];

    // Mount credentials (translate paths for Docker-in-Docker)
    for mount in &config.credential_mounts {
        let host = translate_credential_host_path(&mount.host_path);
        // Check if the path exists from this container's perspective
        let local = expand_tilde(&mount.host_path);
        if !Path::new(&local).exists() {
            continue; // Skip non-existent credential paths
        }
        let mode = if mount.readonly { "ro" } else { "rw" };
        args.push("-v".into());
        args.push(format!("{}:{}:{}", host, mount.container_path, mode));
    }

    // Image
    args.push(image.to_string());

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

    tracing::debug!(args = ?args, "Docker run command");

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

    // Capture container stderr before cleanup (for error diagnosis)
    if exit_code != 0 {
        let stderr_output = Command::new("docker")
            .args(["logs", "--tail", "20", &container_id])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .output()
            .await;
        let stderr_text = stderr_output
            .map(|o| {
                let stdout = String::from_utf8_lossy(&o.stdout);
                let stderr = String::from_utf8_lossy(&o.stderr);
                format!("{}{}", stdout, stderr)
            })
            .unwrap_or_default();
        tracing::error!(exit_code, stderr = %stderr_text.trim(), "Sandbox container failed");
        cleanup_container(&container_id).await;
        return Err(format!("claude exited with code {exit_code}: {}", stderr_text.trim().chars().take(500).collect::<String>()));
    }

    // Clean up container
    cleanup_container(&container_id).await;

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
