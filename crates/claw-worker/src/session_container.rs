//! Session container manager for interactive chat.
//! Keeps Docker containers alive across chat messages for performance.

use crate::docker::{DockerConfig, shell_escape, translate_to_host_path};
use crate::executor::{ExecutionResult, StreamState};
use deadpool_redis::Pool;
use std::path::{Path, PathBuf};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc;
use uuid::Uuid;

/// Ensure a session container is running for the given chat.
/// Returns the container name. Creates the container if not running.
pub async fn ensure_container(
    pool: &Pool,
    chat_id: Uuid,
    workspace_id: Uuid,
    config: &DockerConfig,
) -> Result<String, String> {
    let container_name = format!("claw-chat-{}", chat_id);

    // Check Redis for existing container
    if let Ok(Some(name)) = claw_redis::get_chat_container(pool, chat_id).await {
        // Verify it's actually running
        let check = Command::new("docker")
            .args(["inspect", "--format", "{{.State.Running}}", &name])
            .output()
            .await;
        if let Ok(output) = check {
            if output.status.success() {
                let running = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if running == "true" {
                    tracing::debug!(chat_id = %chat_id, container = %name, "Reusing session container");
                    return Ok(name);
                }
            }
        }
        // Container dead — clean up tracking
        claw_redis::delete_chat_container(pool, chat_id).await.ok();
        // Try to remove the dead container
        Command::new("docker").args(["rm", "-f", &container_name]).output().await.ok();
    }

    // Remove any dead container with the same name
    Command::new("docker").args(["rm", "-f", &container_name]).output().await.ok();

    // Create new container
    let checkout = checkout_path(workspace_id);

    let mut args = vec![
        "run".to_string(),
        "-d".into(),
        "--name".into(),
        container_name.clone(),
        "--memory".into(),
        "1g".into(),
        "--cpus".into(),
        "1.0".into(),
        "--pids-limit".into(),
        "128".into(),
        "-w".into(),
        "/workspace".into(),
        "-e".into(),
        "HOME=/home/claw".into(),
    ];

    // Mount workspace (translate path for Docker-in-Docker)
    let host_checkout = translate_to_host_path(&checkout);
    args.push("-v".into());
    args.push(format!("{}:/workspace", host_checkout));

    // Credential mounts from config (includes .claude, .claude.json, .ssh etc.)
    for mount in &config.credential_mounts {
        // Expand ~ to home directory
        let host_path = if mount.host_path.starts_with("~/") {
            let home = dirs::home_dir().unwrap_or_else(|| "/tmp".into());
            home.join(&mount.host_path[2..]).display().to_string()
        } else {
            mount.host_path.clone()
        };
        // Skip if host path doesn't exist
        if !std::path::Path::new(&host_path).exists() {
            continue;
        }
        args.push("-v".into());
        let mode = if mount.readonly { ":ro" } else { "" };
        args.push(format!("{}:{}{}", host_path, mount.container_path, mode));
    }

    // Image + sleep command
    args.push(config.image.clone());
    args.push("sleep".into());
    args.push("infinity".into());

    let output = Command::new("docker")
        .args(&args)
        .output()
        .await
        .map_err(|e| format!("Failed to start session container: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("docker run failed: {stderr}"));
    }

    // Track in Redis
    claw_redis::set_chat_container(pool, chat_id, &container_name).await.ok();

    tracing::info!(chat_id = %chat_id, container = %container_name, "Session container started");
    Ok(container_name)
}

/// Execute a chat message in an existing session container.
/// Writes the prompt to a runner script, executes via docker exec, parses stream-json output.
pub async fn execute_chat_message(
    container_name: &str,
    workspace_id: Uuid,
    prompt: &str,
    model: Option<&str>,
    log_tx: mpsc::Sender<String>,
) -> Result<ExecutionResult, String> {
    let checkout = checkout_path(workspace_id);
    let start = std::time::Instant::now();

    // Build the runner script with shell-escaped prompt
    // Build the full command string with shell_escape
    let prompt_owned = prompt.to_string();
    let mut cmd_parts: Vec<String> = vec![
        "stdbuf".into(), "-oL".into(),
        "claude".into(), "-p".into(), shell_escape(&prompt_owned),
        "--output-format".into(), "stream-json".into(),
        "--verbose".into(),
        "--dangerously-skip-permissions".into(),
    ];
    if let Some(m) = model {
        cmd_parts.push("--model".into());
        cmd_parts.push(shell_escape(m));
    }
    cmd_parts.push("--max-budget-usd".into());
    cmd_parts.push("10".into());

    let script = format!("#!/bin/bash\ncd /workspace\nexec {}", cmd_parts.join(" "));
    let script_path = checkout.join(".claw-chat-run.sh");
    tokio::fs::write(&script_path, &script)
        .await
        .map_err(|e| format!("Failed to write chat runner script: {e}"))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        tokio::fs::set_permissions(&script_path, std::fs::Permissions::from_mode(0o755)).await.ok();
    }

    // Execute via docker exec
    let mut child = Command::new("docker")
        .args(["exec", container_name, "bash", "/workspace/.claw-chat-run.sh"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| format!("docker exec failed: {e}"))?;

    let stdout = child.stdout.take().ok_or("No stdout")?;
    let stderr = child.stderr.take().ok_or("No stderr")?;

    // Capture stderr in background
    let stderr_handle = tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        let mut output = String::new();
        while let Ok(Some(line)) = lines.next_line().await {
            output.push_str(&line);
            output.push('\n');
        }
        output
    });

    // Parse stream-json from stdout
    let reader = BufReader::new(stdout);
    let mut lines = reader.lines();
    let mut state = StreamState::new();

    while let Ok(Some(line)) = lines.next_line().await {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        state.process_line(trimmed);
        log_tx.send(line.clone()).await.ok();
    }

    let exit = child.wait().await.map_err(|e| format!("docker exec wait: {e}"))?;
    let stderr_output = stderr_handle.await.unwrap_or_default();
    let duration_ms = start.elapsed().as_millis() as u64;

    if !exit.success() {
        // Check if container is still alive
        let alive = Command::new("docker")
            .args(["inspect", "--format", "{{.State.Running}}", container_name])
            .output()
            .await
            .map(|o| String::from_utf8_lossy(&o.stdout).trim() == "true")
            .unwrap_or(false);

        if !alive {
            tracing::warn!(container = %container_name, "Session container died during execution");
            // Don't delete Redis key here — let the caller handle it
        }

        return Err(format!("claude exited with code {}: {}", exit.code().unwrap_or(-1), stderr_output.trim()));
    }

    let (result_text, cost_usd) = state.finalize();
    Ok(ExecutionResult {
        result_text,
        cost_usd,
        duration_ms,
    })
}

/// Stop and remove idle session containers.
pub async fn cleanup_idle_containers(pool: &Pool, timeout_secs: u64) {
    // List all chat sessions and check last_activity
    // This is called periodically from the worker main loop
    let output = Command::new("docker")
        .args(["ps", "--filter", "name=claw-chat-", "--format", "{{.Names}}"])
        .output()
        .await;

    let containers: Vec<String> = match output {
        Ok(o) if o.status.success() => {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .map(|s| s.to_string())
                .collect()
        }
        _ => return,
    };

    for name in containers {
        // Extract chat_id from container name "claw-chat-{uuid}"
        let chat_id_str = name.strip_prefix("claw-chat-").unwrap_or("");
        let chat_id: Uuid = match chat_id_str.parse() {
            Ok(id) => id,
            Err(_) => continue,
        };

        // Check session last_activity
        if let Ok(Some(session)) = claw_redis::get_chat_session(pool, chat_id).await {
            let idle_secs = (chrono::Utc::now() - session.last_activity).num_seconds() as u64;
            if idle_secs > timeout_secs {
                tracing::info!(chat_id = %chat_id, idle_secs, "Stopping idle session container");

                // Git commit before shutdown
                let checkout = checkout_path(session.workspace_id);
                git_commit(&checkout, "chat: auto-commit on idle shutdown").await;

                // Stop and remove
                Command::new("docker").args(["stop", &name]).output().await.ok();
                Command::new("docker").args(["rm", "-f", &name]).output().await.ok();
                claw_redis::delete_chat_container(pool, chat_id).await.ok();
            }
        }
    }
}

/// Clean up orphaned session containers on worker startup.
pub async fn cleanup_orphans(_pool: &Pool) {
    let output = Command::new("docker")
        .args(["ps", "-a", "--filter", "name=claw-chat-", "--format", "{{.Names}}"])
        .output()
        .await;

    if let Ok(o) = output {
        if o.status.success() {
            for name in String::from_utf8_lossy(&o.stdout).lines() {
                tracing::info!(container = %name, "Cleaning up orphaned session container");
                Command::new("docker").args(["rm", "-f", name]).output().await.ok();
            }
        }
    }
}

/// Git commit the workspace.
pub async fn git_commit(checkout: &Path, message: &str) {
    let cmds = format!(
        "cd {} && git add -A && git diff --cached --quiet || git commit -m '{}'",
        checkout.display(),
        message.replace('\'', "'\\''")
    );
    Command::new("bash").args(["-c", &cmds]).output().await.ok();
}

fn checkout_path(workspace_id: Uuid) -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| "/tmp".into());
    home.join(".claw").join("checkouts").join(workspace_id.to_string())
}

