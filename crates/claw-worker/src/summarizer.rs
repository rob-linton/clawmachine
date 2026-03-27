//! Cognitive pipeline: summarization + memory extraction + mood + anticipation.
//! Runs in a dedicated long-running summarizer container, isolated from chat session containers.
//! Each `claude -p` call uses a unique subdirectory to avoid --continue contamination.

use crate::docker::{DockerConfig, expand_tilde, translate_credential_host_path};
use serde::{Deserialize, Serialize};
use std::path::Path;
use tokio::process::Command;

/// Result of the cognitive pipeline.
#[derive(Debug, Clone)]
pub struct CognitiveResult {
    /// One-line exchange summary.
    pub summary: String,
    /// Notebook updates to apply.
    pub notebook_ops: Vec<NotebookOp>,
    /// Mood assessment (productive, debugging, exploring, frustrated, planning, reviewing, casual).
    pub mood: Option<String>,
    /// What the user might need next (injected into next CLAUDE.md).
    pub anticipation: Option<String>,
}

/// A single notebook operation from the cognitive pipeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotebookOp {
    pub op: String,       // "update" | "append" | "create"
    pub file: String,     // Relative path in .notebook/
    pub content: String,  // Full content or text to append
    pub summary: String,  // One-line description
}

/// Internal type for JSON parsing of stage 1+2 output.
#[derive(Debug, Deserialize)]
struct ExtractResult {
    summary: String,
    #[serde(default)]
    notebook_ops: Vec<NotebookOp>,
}

/// Ensure the long-running summarizer container is up.
/// Returns the container name. Safe to call repeatedly — reuses existing container.
pub async fn ensure_summarizer_container(config: &DockerConfig) -> Result<String, String> {
    let container_name = format!("claw-summarizer-{}", std::process::id());

    // Check if already running
    let check = Command::new("docker")
        .args(["inspect", "--format", "{{.State.Running}}", &container_name])
        .output()
        .await;
    if let Ok(output) = check {
        if output.status.success() && String::from_utf8_lossy(&output.stdout).trim() == "true" {
            return Ok(container_name);
        }
    }

    // Remove any dead container with the same name
    Command::new("docker").args(["rm", "-f", &container_name]).output().await.ok();

    // Determine UID/GID
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
        "--memory".into(), "1g".into(),
        "--cpus".into(), "0.5".into(),
        "--pids-limit".into(), "64".into(),
        "-w".into(), "/tmp/summarizer".into(),
        "-e".into(), "HOME=/home/claw".into(),
    ];

    // Mount credentials read-only — summarizer never needs to write to them
    for mount in &config.credential_mounts {
        let host = translate_credential_host_path(&mount.host_path);
        let local = expand_tilde(&mount.host_path);
        if !Path::new(&local).exists() { continue; }
        // Force read-only for summarizer
        args.push("-v".into());
        args.push(format!("{}:{}:ro", host, mount.container_path));
    }

    args.push("--entrypoint".into());
    args.push("sleep".into());
    args.push(config.image.clone());
    args.push("infinity".into());

    let output = Command::new("docker").args(&args).output().await
        .map_err(|e| format!("Failed to start summarizer container: {e}"))?;

    if !output.status.success() {
        return Err(format!("Summarizer docker run failed: {}", String::from_utf8_lossy(&output.stderr)));
    }

    // Create the working directory inside the container with correct ownership.
    // Use bash -c to mkdir + chmod in one exec (the --user flag makes exec run as claw user,
    // but the dir may need to be created as root first if /tmp has restrictive perms).
    // Use -u 0 to run as root, then chown to the container user.
    Command::new("docker")
        .args(["exec", "-u", "0", &container_name, "bash", "-c",
               &format!("mkdir -p /tmp/summarizer && chown {}:{} /tmp/summarizer", uid, gid)])
        .output().await.ok();

    tracing::info!(container = %container_name, "Summarizer container started");
    Ok(container_name)
}

/// Execute a prompt in the summarizer container using a unique subdirectory.
/// Uses base64 encoding to transfer the prompt (avoids stdin piping and docker cp
/// path issues in Docker-in-Docker environments).
async fn run_in_summarizer(container: &str, prompt: &str, model: &str) -> Option<String> {
    // Validate model to prevent shell injection — only allow known model names
    let safe_model = match model {
        "haiku" | "sonnet" | "opus" => model,
        _ => {
            tracing::warn!(model = %model, "Invalid model for summarizer, defaulting to haiku");
            "haiku"
        }
    };
    let subdir = format!("/tmp/summarizer/{}", uuid::Uuid::new_v4());

    // Write prompt and runner script to the container via base64 + docker exec.
    // We write a bash script that reads the prompt from a file, avoiding shell
    // escaping issues with $(cat) expansion and special characters in the prompt.
    use base64::Engine;
    let b64_prompt = base64::engine::general_purpose::STANDARD.encode(prompt.as_bytes());

    // Build the runner script content (this is safe — no user input in the script itself)
    let runner_script = format!(
        "#!/bin/bash\ncd {subdir}\nclaude -p \"$(cat prompt.txt)\" --model {model} --output-format text\n",
        subdir = subdir,
        model = safe_model,
    );
    let b64_script = base64::engine::general_purpose::STANDARD.encode(runner_script.as_bytes());

    // Write both files in one docker exec call
    let setup_cmd = format!(
        "mkdir -p {subdir} && echo '{b64_prompt}' | base64 -d > {subdir}/prompt.txt && echo '{b64_script}' | base64 -d > {subdir}/run.sh && chmod +x {subdir}/run.sh",
        subdir = subdir,
        b64_prompt = b64_prompt,
        b64_script = b64_script,
    );

    let write_result = Command::new("docker")
        .args(["exec", container, "bash", "-c", &setup_cmd])
        .output().await;

    match &write_result {
        Err(e) => {
            tracing::warn!(error = %e, "Failed to write prompt to summarizer container");
            return None;
        }
        Ok(out) if !out.status.success() => {
            tracing::warn!(
                stderr = %String::from_utf8_lossy(&out.stderr).chars().take(200).collect::<String>(),
                "Summarizer prompt write failed"
            );
            return None;
        }
        _ => {}
    }

    // Execute the runner script (not the prompt directly — avoids shell expansion issues)
    let output = match Command::new("docker")
        .args(["exec", container, "bash", &format!("{}/run.sh", subdir)])
        .output()
        .await
    {
        Ok(out) => out,
        Err(e) => {
            tracing::warn!(error = %e, "Summarizer docker exec failed");
            // Clean up
            Command::new("docker")
                .args(["exec", container, "rm", "-rf", &subdir])
                .output().await.ok();
            return None;
        }
    };

    // Clean up the subdir
    Command::new("docker")
        .args(["exec", container, "rm", "-rf", &subdir])
        .output().await.ok();

    if !output.status.success() {
        tracing::warn!(
            exit_code = ?output.status.code(),
            stdout = %String::from_utf8_lossy(&output.stdout).chars().take(200).collect::<String>(),
            stderr = %String::from_utf8_lossy(&output.stderr).chars().take(200).collect::<String>(),
            "Summarizer claude -p failed"
        );
        return None;
    }

    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if text.is_empty() {
        let stderr_text = String::from_utf8_lossy(&output.stderr);
        tracing::warn!(stderr = %stderr_text.chars().take(300).collect::<String>(), "Summarizer returned empty output");
        None
    } else {
        tracing::info!(output_len = text.len(), "Summarizer call succeeded");
        Some(text)
    }
}

/// Run the full cognitive pipeline on a chat exchange.
pub async fn run_cognitive_pipeline(
    container: &str,
    user_msg: &str,
    assistant_resp: &str,
    notebook_index: &str,
    changed_files: &[String],
    recent_moods: &[String],
    seq: u32,
) -> Option<CognitiveResult> {
    let user_short = truncate(user_msg, 2000);
    let assistant_short = truncate(assistant_resp, 2000);
    let changed = if changed_files.is_empty() {
        "(none)".to_string()
    } else {
        changed_files.join(", ")
    };

    // --- Stage 1+2: Extract & Connect ---
    let extract_prompt = format!(
        r#"Analyze this chat exchange in the context of the user's existing notebook.

EXISTING NOTEBOOK ENTRIES:
{notebook_index}

FILES ALREADY UPDATED BY ASSISTANT (skip these):
{changed}

EXCHANGE:
User: {user_short}
Assistant: {assistant_short}

Output JSON:
{{"summary": "One sentence, max 100 chars, what was discussed/decided", "notebook_ops": [{{"op": "update|append|create", "file": "relative/path.md", "content": "full content or append text", "summary": "one-line"}}]}}

Rules:
- Only create notebook_ops for genuinely important information (decisions, facts about the user, project context)
- Prefer "update" over "create" — update existing files when the topic already has a file
- Use "append" to add a line to decisions.md or timeline.md
- Skip files listed in ALREADY UPDATED
- Output valid JSON only. Empty notebook_ops array is fine for routine exchanges."#,
    );

    let stage1_raw = run_in_summarizer(container, &extract_prompt, "haiku").await?;
    let stage1 = parse_extract_result(&stage1_raw);

    let summary = stage1.as_ref().map(|s| s.summary.clone())
        .unwrap_or_else(|| {
            // Fallback: use first 100 chars of user message as summary
            format!("User: {}", &user_msg.chars().take(80).collect::<String>())
        });
    let notebook_ops = stage1.map(|s| s.notebook_ops).unwrap_or_default();

    // --- Stage 3: Reflect (mood) ---
    let moods_str = if recent_moods.is_empty() {
        "none yet".to_string()
    } else {
        recent_moods.join(", ")
    };
    let reflect_prompt = format!(
        "Given this chat exchange and recent mood history, assess the conversation mood.\n\n\
         Recent moods: {moods_str}\n\
         User said: {user_short}\n\
         Assistant said: {assistant_short}\n\n\
         Output ONE WORD: productive | debugging | exploring | frustrated | planning | reviewing | casual",
    );

    let mood = run_in_summarizer(container, &reflect_prompt, "haiku").await
        .map(|s| {
            let word = s.trim().to_lowercase();
            // Validate it's one of our expected values
            match word.as_str() {
                "productive" | "debugging" | "exploring" | "frustrated" |
                "planning" | "reviewing" | "casual" => word,
                _ => "productive".to_string(), // default
            }
        });

    // --- Stage 4: Anticipate (every 5th message) ---
    let anticipation = if seq % 5 == 0 && seq > 0 {
        let anticipate_prompt = format!(
            "Based on this conversation, what might the user need or ask about next? \
             Write 1-2 sentences to help their assistant be prepared.\n\n\
             Notebook: {notebook_index}\n\
             Latest exchange: {summary}\n\
             Mood: {mood}",
            mood = mood.as_deref().unwrap_or("unknown"),
        );
        run_in_summarizer(container, &anticipate_prompt, "haiku").await
    } else {
        None
    };

    Some(CognitiveResult {
        summary,
        notebook_ops,
        mood,
        anticipation,
    })
}

/// Generate/update the rolling summary.
/// Writes to .chat/summary.md in workspace.
pub async fn update_rolling_summary(
    container: &str,
    pool: &deadpool_redis::Pool,
    chat_id: uuid::Uuid,
    workspace_id: uuid::Uuid,
) -> Result<(), String> {
    let msgs = claw_redis::get_all_chat_messages(pool, chat_id).await
        .map_err(|e| format!("Failed to load messages: {e}"))?;

    let summaries: Vec<String> = msgs.iter()
        .filter_map(|m| m.summary.as_ref().map(|s| format!("[{}] {}: {}", m.seq, m.role, s)))
        .collect();

    if summaries.is_empty() {
        return Ok(());
    }

    let prompt = format!(
        "Here are summaries of all messages in a conversation.\n\
         Write a concise rolling summary (max 500 words) covering:\n\
         - Key topics discussed\n\
         - Important decisions made and their rationale\n\
         - Current state of any work in progress\n\
         - Any deadlines or commitments mentioned\n\n\
         Message summaries:\n{}\n\n\
         Output the summary text directly, no JSON or formatting.",
        summaries.join("\n")
    );

    let summary = run_in_summarizer(container, &prompt, "haiku").await
        .ok_or_else(|| "Rolling summary generation failed".to_string())?;

    let checkout = crate::session_container::checkout_path(workspace_id);
    tokio::fs::create_dir_all(checkout.join(".chat")).await.ok();
    tokio::fs::write(checkout.join(".chat/summary.md"), &summary).await
        .map_err(|e| format!("Failed to write summary: {e}"))?;

    tracing::info!(chat_id = %chat_id, len = summary.len(), "Updated rolling summary");
    Ok(())
}

/// Run memory consolidation (the "thinking between messages" step).
/// Called before cleaning up an idle chat container.
pub async fn consolidate_notebook(
    pool: &deadpool_redis::Pool,
    username: &str,
    container: &str,
) -> Result<(), String> {
    let entries = claw_redis::memory::list_notebook_files(pool, username).await
        .map_err(|e| format!("Failed to list notebook: {e}"))?;

    if entries.len() < 3 {
        return Ok(()); // Not enough to consolidate
    }

    let mut notebook_dump = String::new();
    for path in &entries {
        if let Ok(Some(entry)) = claw_redis::memory::get_notebook_entry(pool, username, path).await {
            notebook_dump.push_str(&format!("=== {} ===\n{}\n\n", path, entry.content));
        }
    }

    let prompt = format!(
        r#"You are reviewing a notebook about a user you work with regularly.
Your job is to consolidate and improve these notes.

CURRENT NOTEBOOK:
{notebook_dump}

Tasks:
1. Merge any duplicate or overlapping entries
2. Update about-user.md with a fresh synthesis of who this person is
3. Update active-projects.md with current status of each project
4. Move scratch.md notes into appropriate topic files, then clear scratch.md
5. Remove any entries that are no longer relevant

Output JSON:
{{"ops": [{{"op": "update|delete", "file": "path.md", "content": "new content", "summary": "one-line"}}]}}"#,
    );

    // Use sonnet for consolidation — infrequent but needs quality
    let result_raw = run_in_summarizer(container, &prompt, "sonnet").await
        .ok_or_else(|| "Consolidation failed".to_string())?;

    // Parse and apply
    if let Some(ops) = parse_consolidation_result(&result_raw) {
        let now = chrono::Utc::now();
        for op in &ops {
            match op.op.as_str() {
                "update" | "create" => {
                    let entry = claw_redis::memory::NotebookEntry {
                        content: op.content.clone(),
                        summary: op.summary.clone(),
                        created: claw_redis::memory::get_notebook_entry(pool, username, &op.file).await
                            .ok().flatten().map(|e| e.created).unwrap_or(now),
                        updated: now,
                        access_count: 0,
                        last_accessed: now,
                    };
                    claw_redis::memory::upsert_notebook_entry(pool, username, &op.file, &entry).await.ok();
                }
                "delete" => {
                    claw_redis::memory::delete_notebook_entry(pool, username, &op.file).await.ok();
                }
                _ => {}
            }
        }

        // Update consolidation timestamp
        let mut meta = claw_redis::memory::get_notebook_meta(pool, username).await
            .ok().flatten().unwrap_or_default();
        meta.last_consolidation = Some(now);
        meta.total_entries = claw_redis::memory::list_notebook_files(pool, username).await
            .map(|f| f.len() as u32).unwrap_or(0);
        claw_redis::memory::set_notebook_meta(pool, username, &meta).await.ok();

        tracing::info!(username = %username, ops = ops.len(), "Notebook consolidated");
    }

    Ok(())
}

// =============================================================================
// JSON parsing with fallbacks
// =============================================================================

fn parse_extract_result(raw: &str) -> Option<ExtractResult> {
    // 1. Try direct parse
    if let Ok(r) = serde_json::from_str::<ExtractResult>(raw) {
        return Some(r);
    }
    // 2. Strip markdown fences
    let stripped = strip_code_fences(raw);
    if let Ok(r) = serde_json::from_str::<ExtractResult>(&stripped) {
        return Some(r);
    }
    // 3. Try to extract summary via regex as minimum
    if let Some(summary) = extract_summary_field(raw) {
        return Some(ExtractResult { summary, notebook_ops: vec![] });
    }
    tracing::warn!(raw_len = raw.len(), "Failed to parse extract result: {}", &raw.chars().take(200).collect::<String>());
    None
}

#[derive(Debug, Deserialize)]
struct ConsolidationResult {
    ops: Vec<NotebookOp>,
}

fn parse_consolidation_result(raw: &str) -> Option<Vec<NotebookOp>> {
    if let Ok(r) = serde_json::from_str::<ConsolidationResult>(raw) {
        return Some(r.ops);
    }
    let stripped = strip_code_fences(raw);
    if let Ok(r) = serde_json::from_str::<ConsolidationResult>(&stripped) {
        return Some(r.ops);
    }
    tracing::warn!("Failed to parse consolidation result");
    None
}

fn strip_code_fences(s: &str) -> String {
    let s = s.trim();
    // Strip ```json ... ``` or ``` ... ```
    if let Some(start) = s.find("```") {
        let after_fence = &s[start + 3..];
        // Skip optional language tag
        let content_start = after_fence.find('\n').map(|i| i + 1).unwrap_or(0);
        let content = &after_fence[content_start..];
        if let Some(end) = content.rfind("```") {
            return content[..end].trim().to_string();
        }
    }
    s.to_string()
}

fn extract_summary_field(raw: &str) -> Option<String> {
    // Look for "summary": "..." pattern
    let marker = "\"summary\"";
    let pos = raw.find(marker)?;
    let after = &raw[pos + marker.len()..];
    let colon = after.find(':')?;
    let after_colon = after[colon + 1..].trim();
    if after_colon.starts_with('"') {
        let content = &after_colon[1..];
        let end = content.find('"')?;
        return Some(content[..end].to_string());
    }
    None
}

fn truncate(s: &str, max_chars: usize) -> String {
    if s.len() <= max_chars {
        s.to_string()
    } else {
        let mut end = max_chars;
        while !s.is_char_boundary(end) && end > 0 {
            end -= 1;
        }
        format!("{}...", &s[..end])
    }
}
