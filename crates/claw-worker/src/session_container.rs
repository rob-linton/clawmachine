//! Session container manager for interactive chat.
//! Keeps Docker containers alive across chat messages for performance.
//! Uses claude --continue to maintain conversation state natively.

use crate::docker::{DockerConfig, expand_tilde, shell_escape, translate_credential_host_path, translate_to_host_path};
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
            if output.status.success() && String::from_utf8_lossy(&output.stdout).trim() == "true" {
                tracing::debug!(chat_id = %chat_id, "Reusing session container");
                return Ok(name);
            }
        }
        claw_redis::delete_chat_container(pool, chat_id).await.ok();
    }

    // Remove any dead container with the same name
    Command::new("docker").args(["rm", "-f", &container_name]).output().await.ok();

    // Create new container
    let checkout = checkout_path(workspace_id);
    let host_checkout = translate_to_host_path(&checkout);

    // Determine UID/GID — same logic as docker_execute_job
    let (uid, gid) = {
        let current_uid = users::get_current_uid();
        if current_uid == 0 {
            let claude_dir = dirs::home_dir().unwrap_or_else(|| "/home/claw".into()).join(".claude");
            std::fs::metadata(&claude_dir)
                .map(|m| { use std::os::unix::fs::MetadataExt; (m.uid(), m.gid()) })
                .unwrap_or((1000, 1000))
        } else {
            (current_uid, users::get_current_gid())
        }
    };

    let mut args = vec![
        "run".to_string(), "-d".into(),
        "--name".into(), container_name.clone(),
        "--user".into(), format!("{}:{}", uid, gid),
        "--memory".into(), "2g".into(),
        "--cpus".into(), "1.0".into(),
        "--pids-limit".into(), "128".into(),
        "-w".into(), "/workspace".into(),
        "-e".into(), "HOME=/home/claw".into(),
        "-v".into(), format!("{}:/workspace", host_checkout),
    ];

    // Credential mounts — use same DinD translation as docker_execute_job
    for mount in &config.credential_mounts {
        let host = translate_credential_host_path(&mount.host_path);
        let local = expand_tilde(&mount.host_path);
        if !Path::new(&local).exists() { continue; }
        let mode = if mount.readonly { "ro" } else { "rw" };
        args.push("-v".into());
        args.push(format!("{}:{}:{}", host, mount.container_path, mode));
    }

    // Override entrypoint (sandbox has ENTRYPOINT ["claude"])
    args.push("--entrypoint".into());
    args.push("sleep".into());
    args.push(config.image.clone());
    args.push("infinity".into());

    let output = Command::new("docker").args(&args).output().await
        .map_err(|e| format!("Failed to start session container: {e}"))?;

    if !output.status.success() {
        return Err(format!("docker run failed: {}", String::from_utf8_lossy(&output.stderr)));
    }

    claw_redis::set_chat_container(pool, chat_id, &container_name).await.ok();
    tracing::info!(chat_id = %chat_id, container = %container_name, "Session container started");
    Ok(container_name)
}

/// Execute a chat message using `claude -p "msg" --continue`.
/// Claude Code maintains conversation state internally — no prompt assembly needed.
/// First message omits --continue (creates the session).
/// Streams assistant text chunks to Redis pub/sub for real-time UI display.
pub async fn execute_chat_message(
    pool: &Pool,
    chat_id: Uuid,
    container_name: &str,
    workspace_id: Uuid,
    user_message: &str,
    model: Option<&str>,
    is_first_message: bool,
    log_tx: mpsc::Sender<String>,
) -> Result<ExecutionResult, String> {
    let checkout = checkout_path(workspace_id);
    let start = std::time::Instant::now();

    // Build the claude command — just the user message, Claude handles context
    let mut cmd_parts: Vec<String> = vec![
        "stdbuf".into(), "-oL".into(),
        "claude".into(), "-p".into(), shell_escape(user_message),
        "--output-format".into(), "stream-json".into(),
        "--verbose".into(),
        "--dangerously-skip-permissions".into(),
    ];

    // --continue tells Claude to resume the most recent conversation
    // Skip on first message (no prior conversation to continue)
    if !is_first_message {
        cmd_parts.push("--continue".into());
    }

    if let Some(m) = model {
        cmd_parts.push("--model".into());
        cmd_parts.push(shell_escape(m));
    }
    cmd_parts.push("--max-budget-usd".into());
    cmd_parts.push("10".into());

    // Write runner script (avoids CLI arg limits for long messages)
    let script = format!("#!/bin/bash\ncd /workspace\nexec {}", cmd_parts.join(" "));
    let script_path = checkout.join(".claw-chat-run.sh");
    tokio::fs::write(&script_path, &script).await
        .map_err(|e| format!("Failed to write runner script: {e}"))?;
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

    let stderr_handle = tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        let mut output = String::new();
        while let Ok(Some(line)) = lines.next_line().await {
            output.push_str(&line);
            output.push('\n');
        }
        output
    });

    // Parse stream-json from stdout + publish assistant text chunks for streaming UI
    let stream_channel = format!("claw:chat:{}:stream", chat_id);
    let mut lines = BufReader::new(stdout).lines();
    let mut state = StreamState::new();
    while let Ok(Some(line)) = lines.next_line().await {
        let trimmed = line.trim();
        if !trimmed.is_empty() {
            // Publish assistant text chunks for real-time streaming
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(trimmed) {
                if val.get("type").and_then(|t| t.as_str()) == Some("assistant") {
                    if let Some(content) = val.get("message").and_then(|m| m.get("content")).and_then(|c| c.as_array()) {
                        for item in content {
                            if item.get("type").and_then(|t| t.as_str()) == Some("text") {
                                if let Some(text) = item.get("text").and_then(|t| t.as_str()) {
                                    if !text.is_empty() {
                                        let chunk = serde_json::json!({"type": "text", "content": text});
                                        claw_redis::publish_chat_stream(pool, &stream_channel, &chunk.to_string()).await.ok();
                                    }
                                }
                            }
                        }
                    }
                } else if val.get("type").and_then(|t| t.as_str()) == Some("result") {
                    let done = serde_json::json!({"type": "done"});
                    claw_redis::publish_chat_stream(pool, &stream_channel, &done.to_string()).await.ok();
                }
            }
            state.process_line(trimmed);
            log_tx.send(line.clone()).await.ok();
        }
    }

    let exit = child.wait().await.map_err(|e| format!("docker exec wait: {e}"))?;
    let stderr_output = stderr_handle.await.unwrap_or_default();
    let duration_ms = start.elapsed().as_millis() as u64;

    if !exit.success() {
        return Err(format!("claude exited with code {}: {}",
            exit.code().unwrap_or(-1), stderr_output.trim()));
    }

    let (result_text, cost_usd) = state.finalize();
    Ok(ExecutionResult { result_text, cost_usd, duration_ms })
}

/// Stop and remove idle session containers.
pub async fn cleanup_idle_containers(pool: &Pool, timeout_secs: u64) {
    let output = Command::new("docker")
        .args(["ps", "--filter", "name=claw-chat-", "--format", "{{.Names}}"])
        .output().await;

    let containers: Vec<String> = match output {
        Ok(o) if o.status.success() => {
            String::from_utf8_lossy(&o.stdout).lines().map(|s| s.to_string()).collect()
        }
        _ => return,
    };

    for name in containers {
        let chat_id_str = name.strip_prefix("claw-chat-").unwrap_or("");
        let chat_id: Uuid = match chat_id_str.parse() {
            Ok(id) => id,
            Err(_) => continue,
        };

        if let Ok(Some(session)) = claw_redis::get_chat_session(pool, chat_id).await {
            let idle_secs = (chrono::Utc::now() - session.last_activity).num_seconds() as u64;
            if idle_secs > timeout_secs {
                tracing::info!(chat_id = %chat_id, idle_secs, "Stopping idle session container");
                let checkout = checkout_path(session.workspace_id);
                git_commit(&checkout, "chat: auto-commit on idle shutdown").await;
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
        .output().await;

    if let Ok(o) = output {
        if o.status.success() {
            for name in String::from_utf8_lossy(&o.stdout).lines() {
                tracing::info!(container = %name, "Cleaning up orphaned session container");
                Command::new("docker").args(["rm", "-f", name]).output().await.ok();
            }
        }
    }
}

pub async fn git_commit(checkout: &Path, message: &str) {
    let cmds = format!(
        "cd {} && git add -A && git diff --cached --quiet || git commit -m '{}'",
        checkout.display(), message.replace('\'', "'\\''")
    );
    Command::new("bash").args(["-c", &cmds]).output().await.ok();
}

/// Refresh .chat/available-skills.json and .chat/available-tools.json from Redis.
pub async fn refresh_available_catalog(pool: &Pool, workspace_id: Uuid) {
    let checkout = checkout_path(workspace_id);
    let chat_dir = checkout.join(".chat");

    // Skills
    if let Ok(skills) = claw_redis::list_skills(pool).await {
        let items: Vec<serde_json::Value> = skills.iter()
            .filter(|s| s.enabled)
            .map(|s| serde_json::json!({"id": s.id, "name": s.name, "description": s.description, "tags": s.tags}))
            .collect();
        tokio::fs::write(chat_dir.join("available-skills.json"), serde_json::to_string_pretty(&items).unwrap_or_default()).await.ok();
    }

    // Tools
    if let Ok(tools) = claw_redis::list_tools(pool).await {
        let items: Vec<serde_json::Value> = tools.iter()
            .filter(|t| t.enabled)
            .map(|t| serde_json::json!({"id": t.id, "name": t.name, "description": t.description, "tags": t.tags}))
            .collect();
        tokio::fs::write(chat_dir.join("available-tools.json"), serde_json::to_string_pretty(&items).unwrap_or_default()).await.ok();
    }
}

/// Check for .chat/install-request.json and process it.
pub async fn process_install_requests(pool: &Pool, workspace_id: Uuid, chat_id: Uuid) {
    let checkout = checkout_path(workspace_id);
    let request_path = checkout.join(".chat").join("install-request.json");

    let content = match tokio::fs::read_to_string(&request_path).await {
        Ok(c) if !c.trim().is_empty() => c,
        _ => return,
    };

    // Parse the request
    let req: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => return,
    };

    let item_type = req.get("type").and_then(|v| v.as_str()).unwrap_or("");
    let item_id = req.get("id").and_then(|v| v.as_str()).unwrap_or("");

    if item_id.is_empty() {
        // Remove the request file
        tokio::fs::remove_file(&request_path).await.ok();
        return;
    }

    match item_type {
        "skill" => {
            // Add skill to the chat session
            if let Ok(Some(mut session)) = claw_redis::get_chat_session(pool, chat_id).await {
                if !session.skill_ids.contains(&item_id.to_string()) {
                    session.skill_ids.push(item_id.to_string());
                    session.updated_at = chrono::Utc::now();
                    claw_redis::update_chat_session(pool, &session).await.ok();
                    tracing::info!(chat_id = %chat_id, skill = %item_id, "Skill added to chat session");
                }
            }
        }
        "tool" => {
            if let Ok(Some(mut session)) = claw_redis::get_chat_session(pool, chat_id).await {
                if !session.tool_ids.contains(&item_id.to_string()) {
                    session.tool_ids.push(item_id.to_string());
                    session.updated_at = chrono::Utc::now();
                    claw_redis::update_chat_session(pool, &session).await.ok();
                    tracing::info!(chat_id = %chat_id, tool = %item_id, "Tool added to chat session");
                }
            }
        }
        _ => {}
    }

    // Remove the request file after processing
    tokio::fs::remove_file(&request_path).await.ok();
}

fn checkout_path(workspace_id: Uuid) -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| "/tmp".into())
        .join(".claw").join("checkouts").join(workspace_id.to_string())
}
