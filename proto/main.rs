use redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

#[derive(Debug, Deserialize)]
struct JobInput {
    id: String,
    prompt: String,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    working_dir: Option<String>,
}

#[derive(Debug, Serialize)]
struct JobResult {
    job_id: String,
    status: String,
    result: String,
    cost_usd: f64,
    duration_ms: u64,
}

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    let redis_url = std::env::var("CLAW_REDIS_URL")
        .unwrap_or_else(|_| "redis://127.0.0.1:6379".to_string());

    println!("claw-prototype: connecting to {redis_url}");

    let client = redis::Client::open(redis_url.as_str())
        .expect("Failed to create Redis client");

    let mut conn = client
        .get_multiplexed_async_connection()
        .await
        .expect("Failed to connect to Redis");

    println!("claw-prototype: connected, waiting for jobs on claw:queue:pending...");

    loop {
        // BLPOP blocks until a job arrives (5s timeout then retry)
        let result: Option<(String, String)> = redis::cmd("BLPOP")
            .arg("claw:queue:pending")
            .arg(5.0)
            .query_async(&mut conn)
            .await
            .unwrap_or(None);

        let Some((_key, job_json)) = result else {
            continue; // Timeout, loop back
        };

        let job: JobInput = match serde_json::from_str(&job_json) {
            Ok(j) => j,
            Err(e) => {
                eprintln!("Failed to parse job JSON: {e}\n  raw: {job_json}");
                continue;
            }
        };

        println!("\n--- Job {} claimed ---", job.id);
        println!("  prompt: {}", truncate(&job.prompt, 80));

        // Mark as running
        let _: () = conn
            .set(format!("claw:job:{}:status", job.id), "running")
            .await
            .unwrap_or_default();

        match execute_job(&job).await {
            Ok(result) => {
                println!(
                    "  completed: ${:.4} | {}ms",
                    result.cost_usd, result.duration_ms
                );

                let result_json = serde_json::to_string(&result).unwrap_or_default();
                let _: () = conn
                    .set(format!("claw:job:{}:result", job.id), &result_json)
                    .await
                    .unwrap_or_default();
                let _: () = conn
                    .set(format!("claw:job:{}:status", job.id), "completed")
                    .await
                    .unwrap_or_default();
            }
            Err(e) => {
                eprintln!("  FAILED: {e}");
                let _: () = conn
                    .set(format!("claw:job:{}:status", job.id), "failed")
                    .await
                    .unwrap_or_default();
                let _: () = conn
                    .set(format!("claw:job:{}:error", job.id), &e)
                    .await
                    .unwrap_or_default();
            }
        }
    }
}

async fn execute_job(job: &JobInput) -> Result<JobResult, String> {
    let mut cmd = Command::new("claude");
    cmd.arg("-p").arg(&job.prompt);
    cmd.arg("--output-format").arg("stream-json");
    cmd.arg("--verbose");
    cmd.arg("--dangerously-skip-permissions");

    if let Some(model) = &job.model {
        cmd.arg("--model").arg(model);
    }

    if let Some(dir) = &job.working_dir {
        cmd.current_dir(dir);
    }

    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    cmd.kill_on_drop(true);

    let start = std::time::Instant::now();

    let mut child = cmd.spawn().map_err(|e| format!("Failed to spawn claude: {e}"))?;

    let stdout = child.stdout.take().ok_or("No stdout")?;
    let stderr = child.stderr.take().ok_or("No stderr")?;

    let reader = BufReader::new(stdout);
    let mut lines = reader.lines();

    let mut final_result: Option<serde_json::Value> = None;
    let mut result_text = String::new();
    let mut line_count = 0;

    // Read stderr in background
    let stderr_reader = BufReader::new(stderr);
    let stderr_handle = tokio::spawn(async move {
        let mut stderr_lines = stderr_reader.lines();
        let mut output = String::new();
        while let Ok(Some(line)) = stderr_lines.next_line().await {
            output.push_str(&line);
            output.push('\n');
        }
        output
    });

    // Stream stdout
    while let Ok(Some(line)) = lines.next_line().await {
        line_count += 1;

        if let Ok(val) = serde_json::from_str::<serde_json::Value>(&line) {
            let msg_type = val.get("type").and_then(|t| t.as_str()).unwrap_or("");

            match msg_type {
                "assistant" => {
                    if let Some(text) = val.get("message").and_then(|m| m.as_str()) {
                        print!("  claude: {}...\r", truncate(text, 60));
                    }
                }
                "result" => {
                    final_result = Some(val.clone());
                    if let Some(text) = val.get("result").and_then(|r| r.as_str()) {
                        result_text = text.to_string();
                    }
                }
                _ => {}
            }
        }
    }

    let exit_status = child
        .wait()
        .await
        .map_err(|e| format!("Failed to wait for claude: {e}"))?;

    let duration_ms = start.elapsed().as_millis() as u64;
    let stderr_output = stderr_handle.await.unwrap_or_default();

    if !exit_status.success() {
        return Err(format!(
            "claude exited with code {}: {}",
            exit_status.code().unwrap_or(-1),
            stderr_output.trim()
        ));
    }

    // Extract cost from result
    let cost_usd = final_result
        .as_ref()
        .and_then(|r| r.get("cost_usd"))
        .and_then(|c| c.as_f64())
        .unwrap_or(0.0);

    println!("  lines streamed: {line_count}");

    Ok(JobResult {
        job_id: job.id.clone(),
        status: "completed".to_string(),
        result: result_text,
        cost_usd,
        duration_ms,
    })
}

fn truncate(s: &str, max: usize) -> &str {
    if s.len() <= max {
        s
    } else {
        &s[..max]
    }
}
