use claw_models::{Job, Tool, Workspace};
use sha2::{Digest, Sha256};
use std::path::Path;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::secrets::{credential_load_prelude, render_credentials_for_stdin};

use crate::executor::ExecutionResult;

/// Configuration for Docker-based execution.
#[derive(Clone)]
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
pub fn translate_to_host_path(container_path: &Path) -> String {
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
pub fn translate_credential_host_path(host_path: &str) -> String {
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

/// Build a derived Docker image with the given tools installed.
/// Returns the derived image tag (or base_image if no tools).
/// Images are cached by content hash — rebuilds only happen when tools change.
pub async fn ensure_tool_image(base_image: &str, tools: &[Tool]) -> Result<String, String> {
    if tools.is_empty() {
        return Ok(base_image.to_string());
    }

    // Compute content hash: base_image + sorted tools by id + install_commands
    let mut hasher = Sha256::new();
    hasher.update(base_image.as_bytes());
    let mut sorted_tools: Vec<&Tool> = tools.iter().collect();
    sorted_tools.sort_by(|a, b| a.id.cmp(&b.id));
    for tool in &sorted_tools {
        hasher.update(tool.id.as_bytes());
        hasher.update(tool.install_commands.as_bytes());
    }
    let hash = format!("{:x}", hasher.finalize());
    let tag = format!("claw-tools:{}", &hash[..12]);

    // Check if image already exists
    let check = Command::new("docker")
        .args(["image", "inspect", &tag])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await;

    if let Ok(status) = check {
        if status.success() {
            tracing::info!(tag = %tag, "Derived tool image already exists");
            return Ok(tag);
        }
    }

    // Build derived image
    tracing::info!(tag = %tag, tools = ?sorted_tools.iter().map(|t| &t.id).collect::<Vec<_>>(), "Building derived tool image");

    let temp_dir = tempfile::tempdir().map_err(|e| format!("Failed to create temp dir: {e}"))?;

    // Generate Dockerfile: join multiline install_commands with " && "
    let mut dockerfile = format!("FROM {}\nUSER root\n", base_image);
    for tool in &sorted_tools {
        let joined = tool
            .install_commands
            .lines()
            .map(|l| l.trim())
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .collect::<Vec<_>>()
            .join(" && ");
        if !joined.is_empty() {
            dockerfile.push_str(&format!("RUN {}\n", joined));
        }
    }

    let dockerfile_path = temp_dir.path().join("Dockerfile");
    std::fs::write(&dockerfile_path, &dockerfile)
        .map_err(|e| format!("Failed to write Dockerfile: {e}"))?;

    let build_output = Command::new("docker")
        .args([
            "build",
            "-t",
            &tag,
            "-f",
            &dockerfile_path.to_string_lossy(),
            &temp_dir.path().to_string_lossy(),
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .map_err(|e| format!("Failed to run docker build: {e}"))?;

    if !build_output.status.success() {
        let stderr = String::from_utf8_lossy(&build_output.stderr);
        return Err(format!(
            "Docker build failed for tool image '{}': {}",
            tag,
            stderr.chars().take(1000).collect::<String>()
        ));
    }

    tracing::info!(tag = %tag, "Derived tool image built successfully");
    Ok(tag)
}

/// Execute a job inside a Docker container.
pub async fn docker_execute_job(
    job: &Job,
    working_dir: &Path,
    config: &DockerConfig,
    workspace: Option<&Workspace>,
    tools: &[Tool],
    credential_env_vars: &std::collections::HashMap<String, String>,
    anthropic_api_key: Option<&str>,
    system_prompt: Option<&str>,
    log_tx: mpsc::Sender<String>,
    cancel: CancellationToken,
) -> Result<ExecutionResult, String> {
    let timeout_secs = job.timeout_secs.unwrap_or(1800);
    let timeout = std::time::Duration::from_secs(timeout_secs);
    let prompt = job.assembled_prompt.as_deref().unwrap_or(&job.prompt);

    // Merge: workspace overrides > global config > defaults
    let base_image = workspace
        .and_then(|w| w.base_image.as_deref())
        .unwrap_or(&config.image);

    // Build derived image with tools installed (cached by content hash)
    let image = ensure_tool_image(base_image, tools).await?;
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

    let has_creds = !credential_env_vars.is_empty();

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
        // Defense-in-depth: prevent privilege escalation via setuid binaries.
        // Pairs with --user to make the root drop irreversible.
        "--security-opt".into(),
        "no-new-privileges".into(),
        // Mount workspace (using host path)
        "-v".into(),
        format!("{}:/workspace", host_workspace_path),
    ];

    // When credentials need to be piped via stdin, attach an interactive
    // stdin to the container. We don't allocate a TTY (no -t).
    if has_creds {
        args.push("-i".into());
    }

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

    // Credentials are NOT passed as `-e` flags. They're piped via stdin and
    // sourced inside the runner script's bash bootstrap. This keeps them out
    // of `docker inspect` output. See `crate::secrets`.

    // Inject Anthropic API key if available (bypasses OAuth inside container)
    if let Some(key) = anthropic_api_key {
        args.push("-e".into());
        args.push(format!("ANTHROPIC_API_KEY={}", key));
    }

    // Check if any tools have auth scripts that need to run before claude
    let auth_scripts: Vec<&str> = tools
        .iter()
        .filter_map(|t| t.auth_script.as_deref())
        .filter(|s| !s.trim().is_empty())
        .collect();
    let has_auth_scripts = !auth_scripts.is_empty();

    // The wrapper-script path is taken whenever we need to either run auth
    // scripts before claude OR pipe credentials via stdin. The script
    // contains NO secrets — credentials arrive via the bootstrap that reads
    // from `cat /dev/stdin`.
    let needs_wrapper_script = has_auth_scripts || has_creds;
    let runner_script_path = working_dir.join(".claw-run.sh");

    if needs_wrapper_script {
        let mut claude_cmd = vec![
            "claude".to_string(),
            "-p".into(),
            prompt.to_string(),
            "--output-format".into(),
            "stream-json".into(),
            "--verbose".into(),
            "--dangerously-skip-permissions".into(),
        ];
        if let Some(model) = &job.model {
            claude_cmd.push("--model".into());
            claude_cmd.push(model.clone());
        }
        let budget = job.max_budget_usd.unwrap_or(1000.0);
        claude_cmd.push("--max-budget-usd".into());
        claude_cmd.push(budget.to_string());
        if let Some(allowed) = &job.allowed_tools {
            if !allowed.is_empty() {
                claude_cmd.push("--allowedTools".into());
                claude_cmd.push(allowed.join(","));
            }
        }
        if let Some(sp) = system_prompt {
            claude_cmd.push("--append-system-prompt".into());
            claude_cmd.push(sp.to_string());
        }

        let claude_line = claude_cmd
            .iter()
            .map(|a| shell_escape(a))
            .collect::<Vec<_>>()
            .join(" ");

        let combined_auth = auth_scripts.join("\n");

        // Wrapper script: bash header → credential load (if any) → auth
        // scripts (if any) → exec claude with stdin redirected to /dev/null
        // (the wrapper drained the real stdin via `cat`).
        let script_content = format!(
            "#!/bin/bash\nset -e\n{prelude}{auth}\ncd /workspace\nexec {claude_cmd} < /dev/null\n",
            prelude = credential_load_prelude(has_creds),
            auth = combined_auth,
            claude_cmd = claude_line,
        );

        std::fs::write(&runner_script_path, &script_content)
            .map_err(|e| format!("Failed to write runner script: {e}"))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&runner_script_path, std::fs::Permissions::from_mode(0o755))
                .ok();
        }

        // Override entrypoint and run the wrapper script.
        args.push("--entrypoint".into());
        args.push("/bin/bash".into());
        args.push(image);
        args.push("/workspace/.claw-run.sh".into());
    } else {
        // No auth scripts — use normal ENTRYPOINT ["claude"] with direct args
        args.push(image);

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

        // Default budget
        let budget = job.max_budget_usd.unwrap_or(1000.0);
        args.push("--max-budget-usd".into());
        args.push(budget.to_string());

        // Only restrict tools when the job explicitly specifies a tool list
        if let Some(allowed) = &job.allowed_tools {
            if !allowed.is_empty() {
                args.push("--allowedTools".into());
                args.push(allowed.join(","));
            }
        }

        // Append system prompt with metadata + completion instruction
        if let Some(sp) = system_prompt {
            args.push("--append-system-prompt".into());
            args.push(sp.to_string());
        }
    }

    let start = std::time::Instant::now();

    // Log args but redact credential env vars (values after -e KEY=VALUE)
    let redacted_args: Vec<String> = {
        let mut out = Vec::new();
        let mut skip_next = false;
        for (i, arg) in args.iter().enumerate() {
            if skip_next {
                skip_next = false;
                continue;
            }
            if arg == "-e" && i + 1 < args.len() {
                let next = &args[i + 1];
                if next.starts_with("HOME=") {
                    out.push(arg.clone());
                    out.push(next.clone());
                } else {
                    out.push(arg.clone());
                    if let Some(eq_pos) = next.find('=') {
                        out.push(format!("{}=***", &next[..eq_pos]));
                    } else {
                        out.push("***".into());
                    }
                }
                skip_next = true;
            } else {
                out.push(arg.clone());
            }
        }
        out
    };
    tracing::debug!(args = ?redacted_args, "Docker run command");

    // Run container attached (not detached) — stdout comes directly to us.
    // This avoids Node.js stdout buffering issues that occur in non-TTY mode
    // when using `docker run -d` + `docker logs -f`.
    let mut cmd = Command::new("docker");
    cmd.args(&args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    if has_creds {
        cmd.stdin(Stdio::piped());
    } else {
        cmd.stdin(Stdio::null());
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Failed to start docker container: {e}"))?;

    // Pipe credential payload to the wrapper script's `cat /dev/stdin` BEFORE
    // we begin awaiting on stdout/stderr. We must close (drop) stdin so bash
    // sees EOF and continues past the credential-load step. Order matters
    // here: write → flush → drop, then take stdout/stderr.
    if has_creds {
        let payload = render_credentials_for_stdin(credential_env_vars);
        if let Some(mut stdin) = child.stdin.take() {
            if let Err(e) = stdin.write_all(payload.as_bytes()).await {
                return Err(format!("Failed to write credentials to container stdin: {e}"));
            }
            if let Err(e) = stdin.flush().await {
                return Err(format!("Failed to flush container stdin: {e}"));
            }
            drop(stdin);
        }
    }

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

    // Clean up the wrapper runner script. It contains no secrets but we
    // don't want it lingering in the workspace.
    if needs_wrapper_script {
        let _ = std::fs::remove_file(&runner_script_path);
    }

    if !exit_status.success() {
        let code = exit_status.code().unwrap_or(-1);
        tracing::error!(exit_code = code, stderr = %stderr_output.trim(), "Sandbox container failed");
        return Err(format!("claude exited with code {code}: {}", stderr_output.trim().chars().take(500).collect::<String>()));
    }

    let (result_text, cost_usd, files_written) = state.finalize(false);

    Ok(ExecutionResult {
        result_text,
        cost_usd,
        duration_ms,
        files_written,
        thinking: None,
    })
}

pub fn expand_tilde(path: &str) -> String {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest).to_string_lossy().to_string();
        }
    }
    path.to_string()
}

/// Shell-escape a string for safe inclusion in a bash script.
pub fn shell_escape(s: &str) -> String {
    if s.is_empty() {
        return "''".to_string();
    }
    // If it's safe (no special chars), return as-is
    if s.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == '.' || c == '/' || c == ':' || c == '=') {
        return s.to_string();
    }
    // Otherwise, wrap in single quotes, escaping any existing single quotes
    format!("'{}'", s.replace('\'', "'\\''"))
}
