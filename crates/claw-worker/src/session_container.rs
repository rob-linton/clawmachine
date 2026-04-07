//! Session container manager for interactive chat.
//! Keeps Docker containers alive across chat messages for performance.
//! Uses claude --continue to maintain conversation state natively.

use crate::docker::{DockerConfig, expand_tilde, shell_escape, translate_credential_host_path, translate_to_host_path};
use crate::executor::{ExecutionResult, StreamState};
use crate::secrets::{credential_load_prelude, render_credentials_for_stdin};
use deadpool_redis::Pool;
use std::path::{Path, PathBuf};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

/// Ensure a session container is running for the given chat.
/// Returns (container_name, is_new_container). `is_new_container` is true when
/// a fresh container was created (no prior --continue conversation exists).
/// If workspace tools change, the container is recreated with the new tool image.
pub async fn ensure_container(
    pool: &Pool,
    chat_id: Uuid,
    workspace_id: Uuid,
    config: &DockerConfig,
    api_key: Option<&str>,
    tools: &[claw_models::Tool],
    workspace: Option<&claw_models::Workspace>,
) -> Result<(String, bool), String> {
    let container_name = format!("claw-chat-{}", chat_id);

    // Determine the image: use derived tool image if tools are configured
    let base_image = workspace
        .and_then(|w| w.base_image.as_deref())
        .unwrap_or(&config.image);
    let image = crate::docker::ensure_tool_image(base_image, tools).await
        .unwrap_or_else(|_| base_image.to_string());

    // Check Redis for existing container
    if let Ok(Some(name)) = claw_redis::get_chat_container(pool, chat_id).await {
        // Check if tool image changed — if so, recreate the container
        let stored_image = claw_redis::get_chat_container_tool_image(pool, chat_id).await
            .ok().flatten().unwrap_or_default();
        if !stored_image.is_empty() && stored_image != image {
            tracing::info!(chat_id = %chat_id, old = %stored_image, new = %image, "Tool image changed — recreating container");
            Command::new("docker").args(["stop", &name]).output().await.ok();
            Command::new("docker").args(["rm", "-f", &name]).output().await.ok();
            claw_redis::delete_chat_container(pool, chat_id).await.ok();
        } else {
            // Verify it's actually running
            let check = Command::new("docker")
                .args(["inspect", "--format", "{{.State.Running}}", &name])
                .output()
                .await;
            if let Ok(output) = check {
                if output.status.success() && String::from_utf8_lossy(&output.stdout).trim() == "true" {
                    tracing::debug!(chat_id = %chat_id, "Reusing session container");
                    return Ok((name, false));
                }
            }
            claw_redis::delete_chat_container(pool, chat_id).await.ok();
        }
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

    // Use workspace resource limits or defaults
    let memory = workspace.and_then(|w| w.memory_limit.as_deref()).unwrap_or("2g");
    let cpu_val = workspace.and_then(|w| w.cpu_limit).unwrap_or(1.0);
    let cpu = format!("{}", cpu_val);

    let mut args = vec![
        "run".to_string(), "-d".into(),
        "--name".into(), container_name.clone(),
        "--user".into(), format!("{}:{}", uid, gid),
        "--memory".into(), memory.to_string(),
        "--cpus".into(), cpu.to_string(),
        "--pids-limit".into(), "256".into(),
        // Defense-in-depth: prevent privilege escalation via setuid
        // binaries. Applies to all subsequent docker exec invocations.
        "--security-opt".into(), "no-new-privileges".into(),
        "-w".into(), "/workspace".into(),
        "-e".into(), "HOME=/home/claw".into(),
        "-v".into(), format!("{}:/workspace", host_checkout),
    ];

    // API key fallback (if OAuth is expired and can't refresh)
    if let Some(key) = api_key {
        args.push("-e".into());
        args.push(format!("ANTHROPIC_API_KEY={}", key));
    }

    // Network — allow chat containers to reach the API server
    let network = std::env::var("CLAW_DOCKER_NETWORK").unwrap_or_else(|_| "bridge".to_string());
    args.push("--network".into());
    args.push(network);
    args.push("--add-host=host.docker.internal:host-gateway".into());

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
    args.push(image.clone());
    args.push("infinity".into());

    tracing::info!(chat_id = %chat_id, %image, "Starting session container");
    let output = Command::new("docker").args(&args).output().await
        .map_err(|e| format!("Failed to start session container: {e}"))?;

    if !output.status.success() {
        return Err(format!("docker run failed: {}", String::from_utf8_lossy(&output.stderr)));
    }

    claw_redis::set_chat_container(pool, chat_id, &container_name, Some(&image)).await.ok();
    tracing::info!(chat_id = %chat_id, container = %container_name, "Session container started");
    Ok((container_name, true))
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
    seq: u32,
    log_tx: mpsc::Sender<String>,
    cancel: CancellationToken,
    credential_env_vars: &std::collections::HashMap<String, String>,
    tools: &[claw_models::Tool],
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

    // Runtime env-var exports that need to be available to claude *and*
    // every tool subprocess throughout the conversation. These are NOT tool
    // credentials — they're per-message-minted runtime values that must
    // persist past `exec claude`. They live in the script file because they
    // are short-lived (rotated every message) and Claude needs them.
    let mut env_lines = String::new();
    if let Ok(Some(session)) = claw_redis::get_chat_session(pool, chat_id).await {
        if let Ok(token) = claw_redis::create_session(pool, &session.user_id).await {
            let api_url = std::env::var("CLAW_CHAT_API_URL")
                .unwrap_or_else(|_| "http://host.docker.internal:8080".to_string());
            env_lines.push_str(&format!(
                "export CLAW_API_URL={}\nexport CLAW_SESSION={}\n",
                shell_escape(&api_url), shell_escape(&token),
            ));
            tracing::debug!(chat_id = %chat_id, user = %session.user_id, "Minted API session token for chat");
        }
    }

    // Tool credentials are NOT written into the script. They are piped via
    // `docker exec -i` stdin and sourced by the bootstrap below. This keeps
    // them out of the workspace file (which used to expose plaintext via the
    // workspace file-browser API and got auto-committed by persistent chat
    // workspaces).
    let has_creds = !credential_env_vars.is_empty();

    // Auth scripts — run tool authentication before Claude starts
    let mut auth_lines = String::new();
    for tool in tools {
        if let Some(ref script) = tool.auth_script {
            if !script.trim().is_empty() {
                auth_lines.push_str(&format!("# Auth: {}\n{}\n", tool.name, script));
            }
        }
    }

    // Write runner script (avoids CLI arg limits for long messages). The
    // script contains NO secrets — env_lines holds only the API session
    // token (which Claude needs throughout the conversation) and the
    // credential bootstrap reads its values from stdin at runtime.
    let script = format!(
        "#!/bin/bash\nset -e\n{prelude}{env}{auth}cd /workspace\nexec {cmd} < /dev/null\n",
        prelude = credential_load_prelude(has_creds),
        env = env_lines,
        auth = auth_lines,
        cmd = cmd_parts.join(" "),
    );
    let script_path = checkout.join(".claw-chat-run.sh");
    tokio::fs::write(&script_path, &script).await
        .map_err(|e| format!("Failed to write runner script: {e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        tokio::fs::set_permissions(&script_path, std::fs::Permissions::from_mode(0o755)).await.ok();
    }

    // Execute via `docker exec`. Add `-i` only when we need to pipe
    // credentials, mirroring the docker.rs path.
    let mut cmd = Command::new("docker");
    let exec_args: &[&str] = if has_creds {
        &["exec", "-i"]
    } else {
        &["exec"]
    };
    cmd.args(exec_args)
        .args([container_name, "bash", "/workspace/.claw-chat-run.sh"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    if has_creds {
        cmd.stdin(std::process::Stdio::piped());
    } else {
        cmd.stdin(std::process::Stdio::null());
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("docker exec failed: {e}"))?;

    // Pipe credential payload BEFORE awaiting on stdout/stderr. Order is
    // load-bearing: write → flush → drop, so bash sees EOF on `cat` and
    // proceeds. Credential payloads are tiny (<4 KB) so the OS pipe buffer
    // can absorb them without us draining stdout in parallel.
    if has_creds {
        let payload = render_credentials_for_stdin(credential_env_vars);
        if let Some(mut stdin) = child.stdin.take() {
            if let Err(e) = stdin.write_all(payload.as_bytes()).await {
                return Err(format!("Failed to write credentials to chat stdin: {e}"));
            }
            if let Err(e) = stdin.flush().await {
                return Err(format!("Failed to flush chat stdin: {e}"));
            }
            drop(stdin);
        }
    }

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

    // Parse stream-json from stdout + publish chunks for real-time streaming UI
    let stream_channel = format!("claw:chat:{}:stream", chat_id);
    let mut lines = BufReader::new(stdout).lines();
    let mut state = StreamState::new();
    let mut accumulated_thinking = String::new();
    let container_for_kill = container_name.to_string();

    let read_result = tokio::select! {
        r = async {
            while let Ok(Some(line)) = lines.next_line().await {
                let trimmed = line.trim();
                if trimmed.is_empty() { continue; }

                // Publish real-time chunks for the streaming UI
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(trimmed) {
                    if val.get("type").and_then(|t| t.as_str()) == Some("assistant") {
                        if let Some(content) = val.get("message").and_then(|m| m.get("content")).and_then(|c| c.as_array()) {
                            for item in content {
                                match item.get("type").and_then(|t| t.as_str()) {
                                    Some("text") => {
                                        if let Some(text) = item.get("text").and_then(|t| t.as_str()) {
                                            if !text.is_empty() {
                                                let chunk = serde_json::json!({"type": "text", "content": text, "seq": seq});
                                                claw_redis::publish_chat_stream(pool, &stream_channel, &chunk.to_string()).await.ok();
                                            }
                                        }
                                    }
                                    Some("thinking") => {
                                        if let Some(text) = item.get("thinking").and_then(|t| t.as_str()) {
                                            if !text.is_empty() {
                                                accumulated_thinking.push_str(text);
                                                let chunk = serde_json::json!({"type": "thinking", "content": text, "seq": seq});
                                                claw_redis::publish_chat_stream(pool, &stream_channel, &chunk.to_string()).await.ok();
                                            }
                                        }
                                    }
                                    Some("tool_use") => {
                                        let name = item.get("name").and_then(|n| n.as_str()).unwrap_or("unknown");
                                        let tool_use_id = item.get("id").and_then(|i| i.as_str()).unwrap_or("");
                                        let summary = match name {
                                            "Write" | "Edit" | "Read" => item.get("input")
                                                .and_then(|i| i.get("file_path"))
                                                .and_then(|p| p.as_str())
                                                .unwrap_or("").to_string(),
                                            "Bash" => item.get("input")
                                                .and_then(|i| i.get("command"))
                                                .and_then(|c| c.as_str())
                                                .map(|c| c.chars().take(80).collect::<String>())
                                                .unwrap_or_default(),
                                            _ => String::new(),
                                        };
                                        let chunk = serde_json::json!({
                                            "type": "tool_use",
                                            "tool": name,
                                            "input_summary": summary,
                                            "tool_use_id": tool_use_id,
                                            "seq": seq,
                                        });
                                        claw_redis::publish_chat_stream(pool, &stream_channel, &chunk.to_string()).await.ok();
                                    }
                                    _ => {}
                                }
                            }
                        }
                    } else if val.get("type").and_then(|t| t.as_str()) == Some("user") {
                        // tool_result content blocks come back as `user` messages.
                        // Publish each one to the chat stream so the inline activity
                        // timeline can show what each tool returned.
                        if let Some(content) = val.get("message").and_then(|m| m.get("content")).and_then(|c| c.as_array()) {
                            for item in content {
                                if item.get("type").and_then(|t| t.as_str()) != Some("tool_result") {
                                    continue;
                                }
                                let tool_use_id = item.get("tool_use_id").and_then(|i| i.as_str()).unwrap_or("");
                                let is_error = item.get("is_error").and_then(|b| b.as_bool()).unwrap_or(false);
                                let full = extract_tool_result_text(item.get("content"));
                                // Truncate by char count (not byte count) so we never split a
                                // multi-byte UTF-8 sequence.
                                const TRUNCATE_AT: usize = 500;
                                let char_count = full.chars().count();
                                let truncated = char_count > TRUNCATE_AT;
                                let output: String = if truncated {
                                    full.chars().take(TRUNCATE_AT).collect()
                                } else {
                                    full
                                };
                                let chunk = serde_json::json!({
                                    "type": "tool_result",
                                    "tool_use_id": tool_use_id,
                                    "output": output,
                                    "truncated": truncated,
                                    "is_error": is_error,
                                    "seq": seq,
                                });
                                claw_redis::publish_chat_stream(pool, &stream_channel, &chunk.to_string()).await.ok();
                            }
                        }
                    } else if val.get("type").and_then(|t| t.as_str()) == Some("result") {
                        let done = serde_json::json!({"type": "done", "seq": seq});
                        claw_redis::publish_chat_stream(pool, &stream_channel, &done.to_string()).await.ok();
                    }
                }
                state.process_line(trimmed);
                log_tx.send(line.clone()).await.ok();
            }
            Ok::<(), String>(())
        } => r,

        _ = cancel.cancelled() => {
            // Graceful shutdown: SIGTERM first, then SIGKILL after 2s
            tracing::info!(chat_id = %chat_id, seq, "Cancelling chat message — sending SIGTERM");
            Command::new("docker")
                .args(["exec", &container_for_kill, "pkill", "-TERM", "-f", "claude"])
                .output().await.ok();
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            // SIGKILL if still running
            Command::new("docker")
                .args(["exec", &container_for_kill, "pkill", "-9", "-f", "claude"])
                .output().await.ok();
            // Publish cancelled event to stream
            let cancelled = serde_json::json!({"type": "cancelled", "seq": seq});
            claw_redis::publish_chat_stream(pool, &stream_channel, &cancelled.to_string()).await.ok();
            return Err("Job was cancelled".to_string());
        }
    };
    read_result.map_err(|e| format!("Stream read error: {e}"))?;

    let exit = child.wait().await.map_err(|e| format!("docker exec wait: {e}"))?;
    let stderr_output = stderr_handle.await.unwrap_or_default();
    let duration_ms = start.elapsed().as_millis() as u64;

    // Clean up the runner script after exec completes. It contains no
    // secrets after the credential-via-stdin change, but we don't want
    // workspace clutter.
    tokio::fs::remove_file(&script_path).await.ok();

    if !exit.success() {
        return Err(format!("claude exited with code {}: {}",
            exit.code().unwrap_or(-1), stderr_output.trim()));
    }

    let (result_text, cost_usd, files_written) = state.finalize(true);
    let thinking = if accumulated_thinking.is_empty() { None } else { Some(accumulated_thinking) };
    Ok(ExecutionResult { result_text, cost_usd, duration_ms, files_written, thinking })
}

/// Stop and remove idle session containers.
/// Runs notebook consolidation before cleanup (the "thinking between messages" step).
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

    // Build DockerConfig once for consolidation (not per-container)
    let all_config = claw_redis::get_all_config(pool).await.unwrap_or_default();
    let dc = DockerConfig::from_config(&all_config);

    for name in containers {
        let chat_id_str = name.strip_prefix("claw-chat-").unwrap_or("");
        let chat_id: Uuid = match chat_id_str.parse() {
            Ok(id) => id,
            Err(_) => continue,
        };

        if let Ok(Some(session)) = claw_redis::get_chat_session(pool, chat_id).await {
            let idle_secs = (chrono::Utc::now() - session.last_activity).num_seconds() as u64;
            if idle_secs > timeout_secs {
                tracing::info!(chat_id = %chat_id, idle_secs, "Stopping idle session container — running consolidation first");

                // Harvest any remaining notebook changes before shutdown
                harvest_notebook(pool, &session.user_id, session.workspace_id, 0).await.ok();

                // Run notebook consolidation (the "thinking between messages" step)
                // Uses the summarizer container, not the chat container being shut down
                if let Ok(summarizer) = crate::summarizer::ensure_summarizer_container(&dc).await {
                    crate::summarizer::consolidate_notebook(pool, &session.user_id, &summarizer).await.ok();
                    crate::summarizer::generate_session_digest(pool, &session.user_id, chat_id, &summarizer).await.ok();
                }

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

/// Extract artifacts (code blocks) from assistant response and store in .chat/artifacts/.
/// Returns list of artifact IDs.
pub async fn extract_artifacts(workspace_id: Uuid, seq: u32, response: &str) -> Vec<u32> {
    let checkout = checkout_path(workspace_id);
    let artifacts_dir = checkout.join(".chat").join("artifacts");
    let index_path = artifacts_dir.join("index.json");

    // Parse fenced code blocks: ```language:filename\n...\n```
    let mut artifacts = Vec::new();
    let mut pos = 0;
    let bytes = response.as_bytes();

    // Load existing index
    let mut index: Vec<serde_json::Value> = tokio::fs::read_to_string(&index_path)
        .await
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();

    let next_id = index.iter()
        .filter_map(|v| v.get("id").and_then(|i| i.as_u64()))
        .max()
        .map(|m| m as u32 + 1)
        .unwrap_or(1);
    let mut artifact_id = next_id;

    while pos < response.len() {
        // Find opening ```
        let Some(start) = response[pos..].find("```") else { break };
        let block_start = pos + start + 3;

        // Find the language/filename line (up to \n)
        let line_end = response[block_start..].find('\n').map(|i| block_start + i).unwrap_or(response.len());
        let lang_line = response[block_start..line_end].trim();

        // Find closing ```
        let Some(end) = response[line_end..].find("\n```") else { pos = line_end; continue };
        let code_start = line_end + 1;
        let code_end = line_end + end;
        let code = &response[code_start..code_end];

        pos = code_end + 4; // past closing ```

        // Determine if this is an artifact
        let (language, filename) = if lang_line.contains(':') {
            let parts: Vec<&str> = lang_line.splitn(2, ':').collect();
            (parts[0].to_string(), Some(parts[1].trim().to_string()))
        } else if lang_line.contains(' ') {
            // Also support "python fibonacci.py" (space-separated)
            let parts: Vec<&str> = lang_line.splitn(2, ' ').collect();
            let potential_file = parts[1].trim();
            if potential_file.contains('.') {
                (parts[0].to_string(), Some(potential_file.to_string()))
            } else {
                (lang_line.to_string(), None)
            }
        } else {
            (lang_line.to_string(), None)
        };

        // Only extract: has filename hint OR >20 lines
        let line_count = code.lines().count();
        if filename.is_none() && line_count <= 20 {
            continue;
        }

        let fname = filename.unwrap_or_else(|| {
            let ext = match language.as_str() {
                "python" | "py" => "py",
                "javascript" | "js" => "js",
                "typescript" | "ts" => "ts",
                "rust" | "rs" => "rs",
                "go" => "go",
                "java" => "java",
                "bash" | "sh" | "shell" => "sh",
                "yaml" | "yml" => "yaml",
                "json" => "json",
                "sql" => "sql",
                "html" => "html",
                "css" => "css",
                _ => "txt",
            };
            format!("snippet_{}.{}", artifact_id, ext)
        });

        // Write artifact file
        if let Err(e) = tokio::fs::create_dir_all(&artifacts_dir).await {
            tracing::warn!(error = %e, "Failed to create artifacts dir");
            continue;
        }
        let artifact_filename = format!("{:03}-{}", artifact_id, fname);
        let artifact_path = artifacts_dir.join(&artifact_filename);
        if let Err(e) = tokio::fs::write(&artifact_path, code).await {
            tracing::warn!(error = %e, artifact = %artifact_filename, "Failed to write artifact");
            continue;
        }

        // Add to index
        index.push(serde_json::json!({
            "id": artifact_id,
            "seq": seq,
            "filename": fname,
            "language": language,
            "lines": line_count,
            "created_at": chrono::Utc::now().to_rfc3339(),
            "path": format!(".chat/artifacts/{}", artifact_filename),
        }));

        artifacts.push(artifact_id);
        artifact_id += 1;
    }

    if !artifacts.is_empty() {
        // Write updated index
        if let Ok(json) = serde_json::to_string_pretty(&index) {
            tokio::fs::write(&index_path, json).await.ok();
        }
        tracing::info!(count = artifacts.len(), "Extracted artifacts from response");
    }

    artifacts
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
            if let Ok(Some(mut session)) = claw_redis::get_chat_session(pool, chat_id).await {
                if !session.skill_ids.contains(&item_id.to_string()) {
                    session.skill_ids.push(item_id.to_string());
                    session.updated_at = chrono::Utc::now();
                    claw_redis::update_chat_session(pool, &session).await.ok();
                }
            }
            // Deploy skill files to workspace so Claude discovers them immediately
            deploy_skill_to_workspace(pool, &checkout, item_id).await;
            tracing::info!(chat_id = %chat_id, skill = %item_id, "Skill installed + deployed to workspace");
        }
        "tool" => {
            if let Ok(Some(mut session)) = claw_redis::get_chat_session(pool, chat_id).await {
                if !session.tool_ids.contains(&item_id.to_string()) {
                    session.tool_ids.push(item_id.to_string());
                    session.updated_at = chrono::Utc::now();
                    claw_redis::update_chat_session(pool, &session).await.ok();
                }
            }
            // Deploy tool skill_content to workspace if it has a usage guide
            deploy_tool_skill_to_workspace(pool, &checkout, item_id).await;
            tracing::info!(chat_id = %chat_id, tool = %item_id, "Tool installed + deployed to workspace");
        }
        _ => {}
    }

    // Remove the request file after processing
    tokio::fs::remove_file(&request_path).await.ok();
}

/// Deploy a skill's SKILL.md + bundled files to .claude/skills/{id}/ in the workspace.
/// Since the workspace is bind-mounted, writing to the host path makes it visible in the container.
async fn deploy_skill_to_workspace(pool: &Pool, checkout: &Path, skill_id: &str) {
    let skill = match claw_redis::get_skill(pool, skill_id).await {
        Ok(Some(s)) => s,
        _ => return,
    };

    let skill_dir = checkout.join(".claude").join("skills").join(skill_id);
    if skill_dir.exists() {
        return; // Already deployed
    }

    if let Err(e) = tokio::fs::create_dir_all(&skill_dir).await {
        tracing::warn!(skill = %skill_id, error = %e, "Failed to create skill dir");
        return;
    }

    // Write SKILL.md with frontmatter
    let skill_md = format!("---\nname: {}\ndescription: {}\n---\n\n{}", skill.name, skill.description, skill.content);
    tokio::fs::write(skill_dir.join("SKILL.md"), &skill_md).await.ok();

    // Write bundled files
    for (rel_path, content) in &skill.files {
        let file_path = skill_dir.join(rel_path);
        if let Some(parent) = file_path.parent() {
            tokio::fs::create_dir_all(parent).await.ok();
        }
        tokio::fs::write(&file_path, content).await.ok();
    }
}

/// Deploy a tool's skill_content as .claude/skills/tool-{id}/SKILL.md in the workspace.
async fn deploy_tool_skill_to_workspace(pool: &Pool, checkout: &Path, tool_id: &str) {
    let tool = match claw_redis::get_tool(pool, tool_id).await {
        Ok(Some(t)) => t,
        _ => return,
    };

    let content = match tool.skill_content {
        Some(ref c) if !c.is_empty() => c,
        _ => return, // No usage guide to deploy
    };

    let skill_dir = checkout.join(".claude").join("skills").join(format!("tool-{}", tool_id));
    if skill_dir.exists() {
        return;
    }

    if let Err(e) = tokio::fs::create_dir_all(&skill_dir).await {
        tracing::warn!(tool = %tool_id, error = %e, "Failed to create tool skill dir");
        return;
    }

    let skill_md = format!("---\nname: {}\ndescription: {}\n---\n\n{}", tool.name, tool.description, content);
    tokio::fs::write(skill_dir.join("SKILL.md"), &skill_md).await.ok();
}

/// Deploy the built-in Claw Machine API skill to the chat workspace.
/// This gives Claude Code the knowledge and credentials to call the API.
pub async fn deploy_api_skill(workspace_id: Uuid) {
    let checkout = checkout_path(workspace_id);
    let skill_dir = checkout.join(".claude").join("skills").join("claw-api");
    // Always overwrite — the skill content may have been updated
    tokio::fs::create_dir_all(&skill_dir).await.ok();
    tokio::fs::write(skill_dir.join("SKILL.md"), API_SKILL_CONTENT).await.ok();
}

/// Extract the displayable text from a stream-json `tool_result.content`
/// field, which can be either a bare string or an array of content blocks
/// like `[{type: "text", text: "..."}, {type: "image", source: {...}}]`.
/// Walks the array, concatenates `text` items with newlines, ignores
/// images and other non-text blocks. Returns an empty string for
/// missing/null/unsupported content.
fn extract_tool_result_text(content: Option<&serde_json::Value>) -> String {
    match content {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(serde_json::Value::Array(items)) => items
            .iter()
            .filter_map(|i| i.get("text").and_then(|t| t.as_str()))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

pub fn checkout_path(workspace_id: Uuid) -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| "/tmp".into())
        .join(".claw").join("checkouts").join(workspace_id.to_string())
}

// =============================================================================
// Notebook deploy/harvest
// =============================================================================

const NOTEBOOK_README: &str = r#"# Your Notebook

This is your persistent notebook. Everything you write here will be remembered across sessions.

## Files
- `about-user.md` — Who the user is, their role, expertise, communication style
- `active-projects.md` — What they're currently building, status, blockers
- `decisions.md` — Append-only decision log: date, decision, rationale
- `people.md` — Team members, roles, who works on what
- `preferences.md` — Tools, frameworks, response style preferences
- `timeline.md` — Key events and deadlines with dates
- `topics/{name}.md` — Deep notes on specific subjects
- `scratch.md` — Working notes for the current session

Write naturally, like a colleague's notes. Update existing entries rather than creating new ones.
"#;

/// Deploy notebook from Redis to workspace before each message.
pub async fn deploy_notebook(pool: &Pool, username: &str, workspace_id: Uuid) -> Result<(), String> {
    let checkout = checkout_path(workspace_id);
    let notebook_dir = checkout.join(".notebook");
    tokio::fs::create_dir_all(&notebook_dir).await
        .map_err(|e| format!("Failed to create .notebook/: {e}"))?;

    // Write README if it doesn't exist
    let readme_path = notebook_dir.join("README.md");
    if !readme_path.exists() {
        tokio::fs::write(&readme_path, NOTEBOOK_README).await.ok();
    }

    // Deploy all entries from Redis
    let files = claw_redis::memory::list_notebook_files(pool, username).await
        .map_err(|e| format!("Failed to list notebook: {e}"))?;

    for path in &files {
        if let Ok(Some(entry)) = claw_redis::memory::get_notebook_entry(pool, username, path).await {
            let file_path = notebook_dir.join(path);
            if let Some(parent) = file_path.parent() {
                tokio::fs::create_dir_all(parent).await.ok();
            }
            tokio::fs::write(&file_path, &entry.content).await.ok();
        }
    }

    Ok(())
}

/// Harvest notebook changes from workspace after each message.
/// Returns list of changed file paths (so the cognitive pipeline can skip them).
pub async fn harvest_notebook(pool: &Pool, username: &str, workspace_id: Uuid, _seq: u32) -> Result<Vec<String>, String> {
    let checkout = checkout_path(workspace_id);
    let notebook_dir = checkout.join(".notebook");
    if !notebook_dir.exists() {
        return Ok(vec![]);
    }

    let known_files = claw_redis::memory::list_notebook_files(pool, username).await
        .unwrap_or_default();

    let mut changed = Vec::new();

    // Walk .notebook/ directory
    let mut entries = tokio::fs::read_dir(&notebook_dir).await
        .map_err(|e| format!("Failed to read .notebook/: {e}"))?;

    let mut file_paths = Vec::new();
    while let Ok(Some(entry)) = entries.next_entry().await {
        collect_files(&notebook_dir, entry.path(), &mut file_paths).await;
    }

    for abs_path in &file_paths {
        let rel_path = abs_path.strip_prefix(&notebook_dir)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();

        // Skip README.md
        if rel_path == "README.md" { continue; }

        let content = match tokio::fs::read_to_string(abs_path).await {
            Ok(c) => c,
            Err(_) => continue,
        };

        // Check if this is new or modified
        let is_new = !known_files.contains(&rel_path);
        let is_modified = if !is_new {
            if let Ok(Some(existing)) = claw_redis::memory::get_notebook_entry(pool, username, &rel_path).await {
                existing.content != content
            } else {
                true
            }
        } else {
            true
        };

        if is_new || is_modified {
            let now = chrono::Utc::now();
            let entry = claw_redis::memory::NotebookEntry {
                content: content.clone(),
                summary: content.lines().next().unwrap_or("").chars().take(100).collect(),
                created: if is_new { now } else {
                    claw_redis::memory::get_notebook_entry(pool, username, &rel_path).await
                        .ok().flatten().map(|e| e.created).unwrap_or(now)
                },
                updated: now,
                access_count: 0,
                last_accessed: now,
            };
            claw_redis::memory::upsert_notebook_entry(pool, username, &rel_path, &entry).await.ok();
            changed.push(rel_path);
        }
    }

    if !changed.is_empty() {
        tracing::info!(count = changed.len(), files = ?changed, "Harvested notebook changes");
    }

    Ok(changed)
}

/// Recursively collect file paths under a directory.
async fn collect_files(base: &Path, path: PathBuf, out: &mut Vec<PathBuf>) {
    if path.is_dir() {
        if let Ok(mut entries) = tokio::fs::read_dir(&path).await {
            while let Ok(Some(entry)) = entries.next_entry().await {
                Box::pin(collect_files(base, entry.path(), out)).await;
            }
        }
    } else if path.is_file() {
        out.push(path);
    }
}

// =============================================================================
// Dynamic CLAUDE.md
// =============================================================================

/// Build and deploy dynamic CLAUDE.md before each message.
pub async fn deploy_dynamic_claude_md(
    pool: &Pool,
    chat_id: Uuid,
    workspace_id: Uuid,
    username: &str,
) -> Result<(), String> {
    let mut sections = Vec::new();

    // 1. Base instructions
    sections.push(BASE_CHAT_INSTRUCTIONS.to_string());

    // 2. Temporal context
    if let Ok(Some(session)) = claw_redis::get_chat_session(pool, chat_id).await {
        sections.push(build_temporal_context(&session));
    }

    // 3. User profile
    if let Ok(Some(entry)) = claw_redis::memory::get_notebook_entry(pool, username, "about-user.md").await {
        claw_redis::memory::touch_notebook_entry(pool, username, "about-user.md").await.ok();
        sections.push(format!("## About This User\n{}", entry.content));
    }

    // 4. Active projects
    if let Ok(Some(entry)) = claw_redis::memory::get_notebook_entry(pool, username, "active-projects.md").await {
        claw_redis::memory::touch_notebook_entry(pool, username, "active-projects.md").await.ok();
        sections.push(format!("## Active Projects\n{}", entry.content));
    }

    // 5. Top memories by importance score
    if let Ok(files) = claw_redis::memory::list_notebook_files(pool, username).await {
        let mut scored: Vec<(String, f64, String)> = Vec::new();
        for path in &files {
            // Skip files already shown in dedicated sections
            if path == "about-user.md" || path == "active-projects.md" || path == "README.md" || path == "scratch.md" || path.starts_with("sessions/") {
                continue;
            }
            if let Ok(Some(entry)) = claw_redis::memory::get_notebook_entry(pool, username, path).await {
                let score = claw_redis::memory::score_entry(&entry, path);
                scored.push((path.clone(), score, entry.summary.clone()));
            }
        }
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        let top: Vec<String> = scored.iter().take(15)
            .map(|(path, _, summary)| format!("- **{}**: {}", path, summary))
            .collect();
        if !top.is_empty() {
            sections.push(format!("## What You Remember\n{}", top.join("\n")));
        }
    }

    // 6. Anticipation note
    if let Ok(Some(meta)) = claw_redis::memory::get_notebook_meta(pool, username).await {
        if let Some(ref anticipation) = meta.anticipation {
            sections.push(format!("## For This Session\n{}", anticipation));
        }
    }

    // 7. Session index (past conversation digests)
    if let Ok(files) = claw_redis::memory::list_notebook_files(pool, username).await {
        let mut session_files: Vec<String> = files.into_iter()
            .filter(|p| p.starts_with("sessions/"))
            .collect();
        session_files.sort();
        session_files.reverse(); // newest first
        let entries: Vec<String> = session_files.iter().take(10).filter_map(|path| {
            // Extract date and slug from "sessions/2026-03-25-auth-middleware.md"
            let name = path.strip_prefix("sessions/")?.strip_suffix(".md")?;
            let (date, slug) = name.split_once('-').and_then(|_| {
                // Date is first 10 chars (YYYY-MM-DD), slug is the rest
                if name.len() > 11 { Some((&name[..10], &name[11..])) } else { None }
            })?;
            Some(format!("- {}: {} — read `.notebook/{}` for details", date, slug, path))
        }).collect();
        if !entries.is_empty() {
            sections.push(format!(
                "## Past Conversations\nSession digests are available for recall:\n{}\n\nWhen the user references a past conversation, read the relevant digest file for full context.",
                entries.join("\n")
            ));
        }
    }

    // 8. Notebook instructions
    sections.push(NOTEBOOK_INSTRUCTIONS.to_string());

    // 9. Conversation history pointers
    sections.push(CONVERSATION_HISTORY_INSTRUCTIONS.to_string());

    // Write to workspace
    let claude_md = sections.join("\n\n---\n\n");
    let checkout = checkout_path(workspace_id);
    tokio::fs::write(checkout.join("CLAUDE.md"), &claude_md).await
        .map_err(|e| format!("Failed to write CLAUDE.md: {e}"))?;

    Ok(())
}

fn build_temporal_context(session: &claw_models::ChatSession) -> String {
    let now = chrono::Utc::now();
    let last_activity_ago = humanize_duration(now - session.last_activity);
    let created_ago = humanize_duration(now - session.created_at);

    format!(
        "## Temporal Context\n\
         - Today: {}\n\
         - Messages in this conversation: {}\n\
         - Conversation started: {} ago\n\
         - Last activity: {} ago",
        now.format("%A, %B %d, %Y %I:%M %p UTC"),
        session.total_messages,
        created_ago,
        last_activity_ago,
    )
}

fn humanize_duration(d: chrono::Duration) -> String {
    let secs = d.num_seconds().max(0);
    if secs < 60 {
        "just now".to_string()
    } else if secs < 3600 {
        format!("{} minutes", secs / 60)
    } else if secs < 86400 {
        let hours = secs / 3600;
        format!("{} hour{}", hours, if hours == 1 { "" } else { "s" })
    } else {
        let days = secs / 86400;
        format!("{} day{}", days, if days == 1 { "" } else { "s" })
    }
}

const BASE_CHAT_INSTRUCTIONS: &str = r#"# Interactive Chat Workspace

You are in a persistent, ongoing conversation. Each message you receive continues the conversation.
You have full access to the conversation history, your notebook, and all files in this workspace."#;

const NOTEBOOK_INSTRUCTIONS: &str = r#"## Your Notebook

You have a persistent notebook at `.notebook/` in this workspace.

**How to use it:**
- `about-user.md` — Update when you learn about the user's role, expertise, preferences
- `active-projects.md` — Track what they're building, current status, blockers
- `decisions.md` — Log key decisions with date, rationale, and alternatives considered
- `people.md` — Team members, roles, who works on what
- `preferences.md` — Communication style, tools, frameworks they prefer
- `timeline.md` — Key events and deadlines with dates
- `topics/{name}.md` — Deep notes on specific topics discussed at length
- `scratch.md` — Working notes for this session (will be consolidated later)

**Guidelines:**
- Write naturally, like a colleague's notes — not formal documentation
- Update existing entries rather than creating new ones when possible
- Focus on what will help you serve this user better in future conversations
- Your notebook persists across sessions — anything you write here, you'll remember forever"#;

const CONVERSATION_HISTORY_INSTRUCTIONS: &str = r#"## Conversation History

- **Recent messages** are in your conversation context (via --continue)
- **Session digests**: `.notebook/sessions/` — narrative recaps of past conversations by date/topic
- **Full message history**: `.chat/messages/` (searchable with `grep -rl "keyword" .chat/messages/`)
- **Rolling summary**: `.chat/summary.md`

When the user references a past conversation ("remember when we discussed..."), read the relevant session digest first, then search .chat/messages/ for detail if needed."#;

// =============================================================================
// Rehydration ("Previously On...")
// =============================================================================

/// Build a "Previously On..." narrative for container restarts.
/// Called when `is_new_container && seq > 1` — Claude has no --continue history
/// but the user has been chatting. Returns the user's message wrapped in a context preamble.
pub async fn build_rehydration_prompt(
    pool: &Pool,
    chat_id: Uuid,
    workspace_id: Uuid,
    username: &str,
    user_message: &str,
) -> String {
    let mut context_parts = Vec::new();

    // 1. Notebook synthesis
    if let Ok(Some(about)) = claw_redis::memory::get_notebook_entry(pool, username, "about-user.md").await {
        context_parts.push(format!("About the user: {}", about.summary));
    }
    if let Ok(Some(projects)) = claw_redis::memory::get_notebook_entry(pool, username, "active-projects.md").await {
        context_parts.push(format!("Active projects: {}", projects.summary));
    }

    // 2. Rolling summary or recent message summaries
    let summary_path = checkout_path(workspace_id).join(".chat/summary.md");
    if let Ok(summary) = tokio::fs::read_to_string(&summary_path).await {
        if !summary.trim().is_empty() {
            context_parts.push(format!("Conversation summary:\n{}", summary));
        }
    } else {
        // Fall back to recent message summaries
        if let Ok(msgs) = claw_redis::get_chat_messages(pool, chat_id, 0, 20).await {
            let summaries: Vec<String> = msgs.iter()
                .filter_map(|m| m.summary.as_ref().map(|s| format!("[{}] {}: {}", m.seq, m.role, s)))
                .collect();
            if !summaries.is_empty() {
                context_parts.push(format!("Recent exchanges:\n{}", summaries.join("\n")));
            }
        }
    }

    // 3. Last 3 full message pairs
    if let Ok(recent) = claw_redis::get_chat_messages(pool, chat_id, 0, 6).await {
        if !recent.is_empty() {
            let recent_text: Vec<String> = recent.iter()
                .map(|m| {
                    let content = if m.content.len() > 500 { &m.content[..500] } else { &m.content };
                    format!("{} [{}]: {}", m.role.to_uppercase(), m.seq, content)
                })
                .collect();
            context_parts.push(format!("Last few messages:\n{}", recent_text.join("\n\n")));
        }
    }

    if context_parts.is_empty() {
        // No context available — just pass through the user message
        return user_message.to_string();
    }

    format!(
        "[You are resuming an ongoing conversation. Your notebook at .notebook/ has been restored \
         with everything you've learned about this user. Here's a quick recap:]\n\n\
         {}\n\n\
         [The user's new message follows. Respond naturally — don't mention this recap unless asked.]\n\n\
         {}",
        context_parts.join("\n\n"),
        user_message
    )
}

const API_SKILL_CONTENT: &str = r##"---
name: Claw Machine API
description: Access the Claw Machine API to manage jobs, skills, tools, workspaces, crons, and pipelines
---

# Claw Machine API

You have access to the Claw Machine API. Use it to manage jobs, skills, tools, workspaces, schedules, and pipelines on behalf of the user.

## Authentication

Two environment variables are set automatically:
- `CLAW_API_URL` — base URL of the API server
- `CLAW_SESSION` — your session token (scoped to the logged-in user)

Include the session cookie on every request:

```bash
curl -s -b "claw_session=$CLAW_SESSION" "$CLAW_API_URL/api/v1/..."
```

For POST/PUT requests add `-H "Content-Type: application/json"` and `-d '{...}'`.

Use `jq` for readable output: `... | jq .`

## Verify Access

Before making API calls, verify your authentication:

```bash
curl -s -b "claw_session=$CLAW_SESSION" "$CLAW_API_URL/api/v1/auth/me" | jq .
```

## IMPORTANT RESTRICTIONS

- **DO NOT** call any `/api/v1/chat/*` endpoints — these are for the chat system itself and calling them could create infinite loops.
- **DO NOT** submit jobs targeting this chat workspace — it could cause lock contention.
- When creating jobs, use a different workspace or let the system assign one.

## API Reference

### System Status
- `GET /api/v1/status` — health check, queue depths, worker count

### Jobs
- `POST /api/v1/jobs` — submit a new job
  ```json
  {"prompt": "...", "workspace_id": "uuid", "skill_ids": [], "model": "sonnet", "priority": "normal", "tags": []}
  ```
- `GET /api/v1/jobs` — list jobs (`?limit=N&offset=N&status=pending|running|completed|failed`)
- `GET /api/v1/jobs/{id}` — get job details
- `GET /api/v1/jobs/{id}/result` — get job result text
- `GET /api/v1/jobs/{id}/logs` — get execution logs
- `POST /api/v1/jobs/{id}/cancel` — cancel a running job
- `DELETE /api/v1/jobs/{id}` — delete a job

### Skills
- `GET /api/v1/skills` — list all skills
- `POST /api/v1/skills` — create a skill
  ```json
  {"id": "my-skill", "name": "My Skill", "content": "# Instructions\n...", "description": "What it does", "tags": ["tag1"]}
  ```
- `GET /api/v1/skills/{id}` — get skill details
- `PUT /api/v1/skills/{id}` — update a skill (same body as create)
- `DELETE /api/v1/skills/{id}` — delete a skill
- `POST /api/v1/skills/install-from-url` — install from git/ZIP URL: `{"url": "https://..."}`

### Tools
- `GET /api/v1/tools` — list all tools
- `POST /api/v1/tools` — create a tool definition
- `GET /api/v1/tools/{id}` — get tool details
- `PUT /api/v1/tools/{id}` — update a tool
- `DELETE /api/v1/tools/{id}` — delete a tool
- `POST /api/v1/tools/install-from-url` — install from git/ZIP URL: `{"url": "https://..."}`

### Workspaces
- `GET /api/v1/workspaces` — list workspaces
- `POST /api/v1/workspaces` — create a workspace
  ```json
  {"name": "...", "description": "...", "persistence_mode": "persistent", "skill_ids": [], "tool_ids": []}
  ```
- `GET /api/v1/workspaces/{id}` — get workspace details
- `PUT /api/v1/workspaces/{id}` — update workspace
- `DELETE /api/v1/workspaces/{id}` — delete workspace
- `GET /api/v1/workspaces/{id}/files` — list files
- `GET /api/v1/workspaces/{id}/files/{path}` — read a file
- `PUT /api/v1/workspaces/{id}/files/{path}` — write a file: `{"content": "..."}`
- `DELETE /api/v1/workspaces/{id}/files/{path}` — delete a file
- `POST /api/v1/workspaces/{id}/fork` — fork a workspace
- `GET /api/v1/workspaces/{id}/history` — git log
- `GET /api/v1/workspaces/{id}/events` — event timeline

### Job Templates
- `GET /api/v1/job-templates` — list templates
- `POST /api/v1/job-templates` — create a template
  ```json
  {"name": "...", "prompt": "...", "skill_ids": [], "tool_ids": [], "workspace_id": "uuid", "model": "sonnet"}
  ```
- `GET /api/v1/job-templates/{id}` — get template
- `PUT /api/v1/job-templates/{id}` — update template
- `DELETE /api/v1/job-templates/{id}` — delete template
- `POST /api/v1/job-templates/{id}/run` — run template as a job

### Cron Schedules
- `GET /api/v1/crons` — list schedules
- `POST /api/v1/crons` — create a schedule
  ```json
  {"name": "...", "schedule": "0 9 * * *", "template_id": "uuid", "enabled": true}
  ```
- `GET /api/v1/crons/{id}` — get schedule
- `PUT /api/v1/crons/{id}` — update schedule
- `DELETE /api/v1/crons/{id}` — delete schedule
- `POST /api/v1/crons/{id}/trigger` — trigger immediately

### Pipelines
- `GET /api/v1/pipelines` — list pipelines
- `POST /api/v1/pipelines` — create a pipeline
  ```json
  {"name": "...", "steps": [{"name": "Step 1", "template_id": "uuid"}]}
  ```
- `GET /api/v1/pipelines/{id}` — get pipeline
- `PUT /api/v1/pipelines/{id}` — update pipeline
- `DELETE /api/v1/pipelines/{id}` — delete pipeline
- `POST /api/v1/pipelines/{id}/run` — run pipeline
- `GET /api/v1/pipeline-runs` — list runs
- `GET /api/v1/pipeline-runs/{id}` — get run details

### Credentials
- `GET /api/v1/credentials` — list credentials (values masked)
- `POST /api/v1/credentials` — create credential
- `PUT /api/v1/credentials/{id}` — update credential values
- `DELETE /api/v1/credentials/{id}` — delete credential

### Configuration
- `GET /api/v1/config` — get all system config
- `PUT /api/v1/config` — update config (partial merge)
- `GET /api/v1/config/{key}` — get single value
- `PUT /api/v1/config/{key}` — set single value

### Docker Management
- `GET /api/v1/docker/status` — Docker daemon status
- `GET /api/v1/docker/images` — list sandbox images

### Catalog
- `POST /api/v1/catalog/sync` — sync skills/tools from catalog repo

## Examples

### List recent jobs
```bash
curl -s -b "claw_session=$CLAW_SESSION" "$CLAW_API_URL/api/v1/jobs?limit=5" | jq '.[] | {id, status, prompt: .prompt[:60]}'
```

### Submit a job
```bash
curl -s -b "claw_session=$CLAW_SESSION" \
  -H "Content-Type: application/json" \
  -d '{"prompt": "Write a hello world Python script", "model": "sonnet"}' \
  "$CLAW_API_URL/api/v1/jobs" | jq .
```

### Check job result
```bash
curl -s -b "claw_session=$CLAW_SESSION" "$CLAW_API_URL/api/v1/jobs/JOB_ID/result" | jq .
```

### Create a skill
```bash
curl -s -b "claw_session=$CLAW_SESSION" \
  -H "Content-Type: application/json" \
  -d '{"id": "my-skill", "name": "My Skill", "content": "# Instructions\nDo the thing.", "description": "A custom skill"}' \
  "$CLAW_API_URL/api/v1/skills" | jq .
```

### List workspaces
```bash
curl -s -b "claw_session=$CLAW_SESSION" "$CLAW_API_URL/api/v1/workspaces" | jq '.[] | {id, name, persistence_mode}'
```

## Managing Tools and Credentials in Chat

Your chat container can have CLI tools (aws, az, gh, etc.) installed directly, with credentials injected automatically. This eliminates the need to submit jobs for simple CLI commands.

### Quick install via file (preferred)
Write a JSON file to request tool or skill installation. The system processes it after your message:
```bash
# Install a tool (by its ID from the tools list)
echo '{"type":"tool","id":"aws-cli-prod-audit"}' > /workspace/.chat/install-request.json

# Install a skill
echo '{"type":"skill","id":"my-skill"}' > /workspace/.chat/install-request.json
```
After writing the install request, tell the user the tool will be available on the next message (the container rebuilds automatically with the new tools).

### List available tools and credentials
```bash
# List all tools
curl -s -b "claw_session=$CLAW_SESSION" "$CLAW_API_URL/api/v1/tools" | jq '.[] | {id, name, description}'

# List credentials (values masked)
curl -s -b "claw_session=$CLAW_SESSION" "$CLAW_API_URL/api/v1/credentials" | jq '.[] | {id, name}'
```

### Bind a credential to a tool on your workspace
```bash
# Get your workspace ID
WS_ID=$(curl -s -b "claw_session=$CLAW_SESSION" "$CLAW_API_URL/api/v1/chat" | jq -r .workspace_id)

# Update workspace to bind credential to tool
curl -s -b "claw_session=$CLAW_SESSION" \
  -X PUT -H "Content-Type: application/json" \
  -d '{"credential_bindings": {"TOOL_ID": "CREDENTIAL_ID"}}' \
  "$CLAW_API_URL/api/v1/workspaces/$WS_ID" | jq .
```

**Note:** Tool and credential changes take effect on the next message. The container is automatically rebuilt with the updated tools.
"##;
