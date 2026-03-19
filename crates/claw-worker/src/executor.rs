use claw_models::{Job, Tool, Workspace};
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::docker::{self, DockerConfig};

pub struct ExecutionResult {
    pub result_text: String,
    pub cost_usd: f64,
    pub duration_ms: u64,
}

/// Execution backend: local (direct subprocess) or Docker (containerized).
#[derive(Debug, Clone, PartialEq)]
pub enum ExecutionBackend {
    Local,
    Docker,
}

impl ExecutionBackend {
    pub fn from_config_str(s: &str) -> Self {
        match s {
            "docker" => Self::Docker,
            _ => Self::Local,
        }
    }
}

/// Dispatch job execution to the appropriate backend.
pub async fn dispatch_execute(
    backend: &ExecutionBackend,
    job: &Job,
    working_dir: &std::path::Path,
    docker_config: Option<&DockerConfig>,
    workspace: Option<&Workspace>,
    tools: &[Tool],
    credential_env_vars: &std::collections::HashMap<String, String>,
    system_prompt: Option<&str>,
    log_tx: mpsc::Sender<String>,
    cancel: CancellationToken,
) -> Result<ExecutionResult, String> {
    match backend {
        ExecutionBackend::Local => {
            local_execute_job(job, working_dir, system_prompt, log_tx, cancel).await
        }
        ExecutionBackend::Docker => {
            let config = docker_config.ok_or("Docker config not available")?;
            docker::docker_execute_job(job, working_dir, config, workspace, tools, credential_env_vars, system_prompt, log_tx, cancel)
                .await
        }
    }
}

/// State collected while parsing Claude's stream-json output.
/// Shared between local and Docker execution paths.
pub struct StreamState {
    pub result_text: String,
    pub final_result: Option<serde_json::Value>,
    pub assistant_texts: Vec<String>,
    pub files_written: Vec<String>,
}

impl StreamState {
    pub fn new() -> Self {
        Self {
            result_text: String::new(),
            final_result: None,
            assistant_texts: Vec::new(),
            files_written: Vec::new(),
        }
    }

    /// Process a single stream-json line.
    pub fn process_line(&mut self, line: &str) {
        let Ok(val) = serde_json::from_str::<serde_json::Value>(line) else {
            return;
        };

        match val.get("type").and_then(|t| t.as_str()) {
            Some("result") => {
                self.final_result = Some(val.clone());
                if let Some(text) = val.get("result").and_then(|r| r.as_str()) {
                    self.result_text = text.to_string();
                }
            }
            Some("assistant") => {
                if let Some(content) = val
                    .get("message")
                    .and_then(|m| m.get("content"))
                    .and_then(|c| c.as_array())
                {
                    for item in content {
                        match item.get("type").and_then(|t| t.as_str()) {
                            Some("text") => {
                                if let Some(text) = item.get("text").and_then(|t| t.as_str()) {
                                    if !text.is_empty() {
                                        self.assistant_texts.push(text.to_string());
                                    }
                                }
                            }
                            Some("tool_use") => {
                                let name =
                                    item.get("name").and_then(|n| n.as_str()).unwrap_or("");
                                if name == "Write" || name == "Edit" {
                                    if let Some(path) = item
                                        .get("input")
                                        .and_then(|i| i.get("file_path"))
                                        .and_then(|p| p.as_str())
                                    {
                                        self.files_written.push(path.to_string());
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
            _ => {}
        }
    }

    /// Build the final result text and extract cost.
    pub fn finalize(mut self) -> (String, f64) {
        if self.result_text.is_empty() && !self.assistant_texts.is_empty() {
            let mut parts: Vec<String> = Vec::new();
            parts.push(self.assistant_texts.join("\n\n"));

            if !self.files_written.is_empty() {
                self.files_written.sort();
                self.files_written.dedup();
                parts.push(format!(
                    "\n\nFiles created/modified:\n{}",
                    self.files_written
                        .iter()
                        .map(|f| format!("- {}", f))
                        .collect::<Vec<_>>()
                        .join("\n")
                ));
            }

            self.result_text = parts.join("");
            if self.result_text.len() > 50_000 {
                self.result_text.truncate(50_000);
                self.result_text.push_str("\n\n[truncated]");
            }
        }

        let cost_usd = self
            .final_result
            .as_ref()
            .and_then(|r| {
                r.get("total_cost_usd")
                    .and_then(|c| c.as_f64())
                    .or_else(|| r.get("cost_usd").and_then(|c| c.as_f64()))
                    .or_else(|| r.get("total_cost").and_then(|c| c.as_f64()))
                    .or_else(|| {
                        r.get("usage")
                            .and_then(|u| u.get("cost_usd"))
                            .and_then(|c| c.as_f64())
                    })
            })
            .unwrap_or(0.0);

        (self.result_text, cost_usd)
    }
}

/// Execute a job locally (direct subprocess).
/// Times out after job.timeout_secs (default 3600s / 1h).
pub async fn local_execute_job(
    job: &Job,
    working_dir: &std::path::Path,
    system_prompt: Option<&str>,
    log_tx: mpsc::Sender<String>,
    cancel: CancellationToken,
) -> Result<ExecutionResult, String> {
    let timeout_secs = job.timeout_secs.unwrap_or(1800);
    let timeout = std::time::Duration::from_secs(timeout_secs);
    // Use the user's prompt directly — no wrapping
    let prompt = job.assembled_prompt.as_deref().unwrap_or(&job.prompt);

    let mut cmd = Command::new("claude");
    cmd.arg("-p").arg(prompt);
    cmd.arg("--output-format").arg("stream-json");
    cmd.arg("--verbose"); // required by --output-format stream-json
    cmd.arg("--dangerously-skip-permissions");

    if let Some(model) = &job.model {
        cmd.arg("--model").arg(model);
    }

    // Default budget $10 so jobs hit the timeout before the turn limit
    let budget = job.max_budget_usd.unwrap_or(1000.0);
    cmd.arg("--max-budget-usd").arg(budget.to_string());

    // Only restrict tools when the job explicitly specifies a tool list.
    // When not specified, let Claude use all tools.
    if let Some(tools) = &job.allowed_tools {
        if !tools.is_empty() {
            cmd.arg("--allowedTools").arg(tools.join(","));
        }
    }

    // Append system prompt with metadata + completion instruction
    if let Some(sp) = system_prompt {
        cmd.arg("--append-system-prompt").arg(sp);
    }

    cmd.current_dir(working_dir);
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    cmd.kill_on_drop(true);

    let start = std::time::Instant::now();
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Failed to spawn claude: {e}"))?;

    let stdout = child.stdout.take().ok_or("No stdout")?;
    let stderr = child.stderr.take().ok_or("No stderr")?;

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
    let mut state = StreamState::new();

    let output = tokio::select! {
        r = async {
            while let Ok(Some(line)) = lines.next_line().await {
                let _ = log_tx.try_send(line.clone());
                state.process_line(&line);
            }
            child.wait().await
        } => r,

        _ = cancel.cancelled() => {
            child.kill().await.ok();
            return Err("Job was cancelled".to_string());
        }

        _ = tokio::time::sleep(timeout) => {
            child.kill().await.ok();
            return Err(format!("Job timed out after {}s", timeout_secs));
        }
    };

    let exit_status = output.map_err(|e| format!("Wait failed: {e}"))?;
    let duration_ms = start.elapsed().as_millis() as u64;
    let stderr_output = stderr_handle.await.unwrap_or_default();

    if !exit_status.success() {
        return Err(format!(
            "claude exited with code {}: {}",
            exit_status.code().unwrap_or(-1),
            stderr_output.trim()
        ));
    }

    let (result_text, cost_usd) = state.finalize();

    Ok(ExecutionResult {
        result_text,
        cost_usd,
        duration_ms,
    })
}
