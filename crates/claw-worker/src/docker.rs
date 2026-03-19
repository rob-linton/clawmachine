use claw_models::{Job, Workspace};
use std::path::Path;
use std::process::Stdio;
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
    let expanded = expand_tilde(host_path);

    // CLAW_HOST_CLAUDE_HOME overrides ~/.claude and ~/.claude.json paths
    if let Ok(host_claude) = std::env::var("CLAW_HOST_CLAUDE_HOME") {
        let home = dirs::home_dir().unwrap_or_else(|| "/home/claw".into());
        let local_claude = home.join(".claude").to_string_lossy().to_string();

        // ~/.claude directory and contents
        if expanded == local_claude || expanded.starts_with(&format!("{}/", local_claude)) {
            return expanded.replace(&local_claude, &host_claude);
        }

        // ~/.claude.json — derive host path from host claude home's parent dir
        let local_claude_json = home.join(".claude.json").to_string_lossy().to_string();
        if expanded == local_claude_json {
            let host_parent = std::path::Path::new(&host_claude).parent()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|| host_claude.clone());
            return format!("{}/.claude.json", host_parent);
        }
    }

    expanded
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
    // Use UID/GID of the Claude auth directory owner, not the worker process.
    // Worker runs as root (for Docker socket), but Claude Code refuses
    // --dangerously-skip-permissions as root. The ~/.claude dir is mounted
    // from the host and owned by the actual user who authenticated.
    let (uid, gid) = {
        let current_uid = users::get_current_uid();
        if current_uid == 0 {
            // Running as root — find the owner of the .claude credential dir
            let claude_dir = dirs::home_dir()
                .unwrap_or_else(|| "/home/claw".into())
                .join(".claude");
            std::fs::metadata(&claude_dir)
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
        "--rm".into(), // auto-remove on exit
        "--name".into(),
        container_name.clone(),
        "--user".into(),
        uid_gid,
        "--workdir".into(),
        "/workspace".into(),
        // Set HOME so Claude Code finds ~/.claude and ~/.claude.json
        "-e".into(),
        "HOME=/home/claw".into(),
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

    // Run container attached (not detached) — stdout comes directly to us.
    // This avoids Node.js stdout buffering issues that occur in non-TTY mode
    // when using `docker run -d` + `docker logs -f`.
    let mut child = Command::new("docker")
        .args(&args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| format!("Failed to start docker container: {e}"))?;

    tracing::info!(container = %container_name, job_id = %job.id, "Docker container started");

    let stdout = child.stdout.take().ok_or("No stdout")?;
    let stderr = child.stderr.take().ok_or("No stderr")?;

    // Capture stderr in background
    let stderr_reader = BufReader::new(stderr);
    let stderr_handle = tokio::spawn(async move {
        let mut lines = stderr_reader.lines();
        let mut output = String::new();
        while let Ok(Some(line)) = lines.next_line().await {
            output.push_str(&line);
            output.push('\n');
        }
        output
    });

    let reader = BufReader::new(stdout);
    let mut lines = reader.lines();
    let mut state = crate::executor::StreamState::new();

    let container_for_cancel = container_name.clone();
    let container_for_timeout = container_name.clone();

    let output = tokio::select! {
        r = async {
            while let Ok(Some(line)) = lines.next_line().await {
                let _ = log_tx.try_send(line.clone());
                state.process_line(&line);
            }
            child.wait().await
        } => r,

        _ = cancel.cancelled() => {
            Command::new("docker").args(["kill", &container_for_cancel])
                .output().await.ok();
            return Err("Job was cancelled".to_string());
        }

        _ = tokio::time::sleep(timeout) => {
            Command::new("docker").args(["stop", "-t", "5", &container_for_timeout])
                .output().await.ok();
            return Err(format!("Job timed out after {}s", timeout_secs));
        }
    };

    let duration_ms = start.elapsed().as_millis() as u64;
    let exit_status = output.map_err(|e| format!("Wait failed: {e}"))?;
    let stderr_output = stderr_handle.await.unwrap_or_default();

    if !exit_status.success() {
        let code = exit_status.code().unwrap_or(-1);
        tracing::error!(exit_code = code, stderr = %stderr_output.trim(), "Sandbox container failed");
        return Err(format!("claude exited with code {code}: {}", stderr_output.trim().chars().take(500).collect::<String>()));
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
