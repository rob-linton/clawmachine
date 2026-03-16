use claw_models::Job;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

pub struct ExecutionResult {
    pub result_text: String,
    pub cost_usd: f64,
    pub duration_ms: u64,
}

/// Execute a job, streaming log lines to the provided channel.
/// Cancellation is cooperative via the CancellationToken.
/// Times out after job.timeout_secs (default 1800s / 30min).
pub async fn execute_job(
    job: &Job,
    log_tx: mpsc::Sender<String>,
    cancel: CancellationToken,
) -> Result<ExecutionResult, String> {
    let timeout_secs = job.timeout_secs.unwrap_or(1800);
    let timeout = std::time::Duration::from_secs(timeout_secs);
    // Use the assembled prompt (with skill injections) if available, otherwise raw prompt
    let prompt = job.assembled_prompt.as_deref().unwrap_or(&job.prompt);

    let mut cmd = Command::new("claude");
    cmd.arg("-p").arg(prompt);
    cmd.arg("--output-format").arg("stream-json");
    cmd.arg("--verbose");
    cmd.arg("--dangerously-skip-permissions");

    if let Some(model) = &job.model {
        cmd.arg("--model").arg(model);
    }

    if let Some(tools) = &job.allowed_tools {
        if !tools.is_empty() {
            cmd.arg("--allowedTools").arg(tools.join(","));
        }
    }

    let wd = &job.working_dir;
    if wd.exists() && wd.is_dir() {
        cmd.current_dir(wd);
    }

    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    cmd.kill_on_drop(true);

    let start = std::time::Instant::now();
    let mut child = cmd.spawn().map_err(|e| format!("Failed to spawn claude: {e}"))?;

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
    let mut result_text = String::new();
    let mut final_result: Option<serde_json::Value> = None;

    let output = tokio::select! {
        r = async {
            while let Ok(Some(line)) = lines.next_line().await {
                // Forward to log channel (best effort)
                let _ = log_tx.try_send(line.clone());

                if let Ok(val) = serde_json::from_str::<serde_json::Value>(&line) {
                    if val.get("type").and_then(|t| t.as_str()) == Some("result") {
                        final_result = Some(val.clone());
                        if let Some(text) = val.get("result").and_then(|r| r.as_str()) {
                            result_text = text.to_string();
                        }
                    }
                }
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

    let cost_usd = final_result
        .as_ref()
        .and_then(|r| r.get("cost_usd"))
        .and_then(|c| c.as_f64())
        .unwrap_or(0.0);

    Ok(ExecutionResult {
        result_text,
        cost_usd,
        duration_ms,
    })
}
