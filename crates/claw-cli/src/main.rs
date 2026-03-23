mod api_client;

use clap::{Parser, Subcommand};
use uuid::Uuid;

#[derive(Parser)]
#[command(name = "claw", about = "Claw Machine CLI — job queue for Claude Code")]
struct Cli {
    /// API server URL
    #[arg(long, env = "CLAW_API_URL", default_value = "http://127.0.0.1:8080")]
    api_url: String,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Submit a new job
    Submit {
        /// The task prompt
        prompt: String,

        /// Model override (sonnet, opus, haiku)
        #[arg(short, long)]
        model: Option<String>,

        /// Job priority (0-9, default 5)
        #[arg(short, long)]
        priority: Option<u8>,

        /// Job tags
        #[arg(short, long)]
        tag: Vec<String>,

        /// Block until job completes and print result
        #[arg(short, long)]
        wait: bool,
    },
    /// Show queue status or job detail
    Status {
        /// Job ID (omit for queue overview)
        job_id: Option<Uuid>,
    },
    /// Get job result
    Result {
        /// Job ID
        job_id: Uuid,
    },
    /// List jobs
    List {
        /// Filter by status
        #[arg(long)]
        status: Option<String>,

        /// Max results
        #[arg(long, default_value = "20")]
        limit: usize,
    },
    /// View job logs
    Logs {
        /// Job ID
        job_id: Uuid,
    },
    /// Cancel a pending or running job
    Cancel {
        /// Job ID
        job_id: Uuid,
    },
    /// Manage skills
    Skill {
        #[command(subcommand)]
        command: SkillCommands,
    },
}

#[derive(Subcommand)]
enum SkillCommands {
    /// Create a new skill
    Create {
        /// Skill ID (slug)
        #[arg(long)]
        id: String,
        /// Display name
        #[arg(long)]
        name: String,
        /// Type: template, claude_config, or script
        #[arg(long, value_name = "TYPE")]
        r#type: String,
        /// Content (inline)
        #[arg(long, conflicts_with = "file")]
        content: Option<String>,
        /// Read content from file
        #[arg(long, conflicts_with = "content")]
        file: Option<String>,
        /// Description
        #[arg(long, default_value = "")]
        description: String,
        /// Tags (comma-separated)
        #[arg(long, default_value = "")]
        tags: String,
    },
    /// List all skills
    List,
    /// Show a skill's content
    Show {
        /// Skill ID
        id: String,
    },
    /// Delete a skill
    Delete {
        /// Skill ID
        id: String,
    },
}

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();
    let cli = Cli::parse();
    let client = api_client::ApiClient::new(&cli.api_url);

    match cli.command {
        Commands::Submit {
            prompt,
            model,
            priority,
            tag,
            wait,
        } => {
            let req = claw_models::CreateJobRequest {
                prompt,
                skill_ids: vec![],
                tool_ids: vec![],
                skill_tags: vec![],
                working_dir: None,
                model,
                max_budget_usd: None,
                allowed_tools: None,
                output_dest: claw_models::OutputDest::Redis,
                tags: tag,
                priority,
                timeout_secs: None,
                workspace_id: None,
                template_id: None,
            };

            match client.submit_job(&req).await {
                Ok(resp) => {
                    println!("Job submitted: {}", resp.id);
                    println!("Status: {}", resp.status);

                    if wait {
                        println!("Waiting for completion...");
                        match client.wait_for_result(resp.id).await {
                            Ok(result) => {
                                println!("\n{}", result.result);
                                println!(
                                    "\nCost: ${:.4} | Duration: {}ms",
                                    result.cost_usd, result.duration_ms
                                );
                            }
                            Err(e) => eprintln!("Error waiting: {e}"),
                        }
                    }
                }
                Err(e) => eprintln!("Error: {e}"),
            }
        }
        Commands::Status { job_id } => match job_id {
            Some(id) => match client.get_job(id).await {
                Ok(job) => {
                    println!("Job:      {}", job.id);
                    println!("Status:   {}", job.status);
                    println!("Created:  {}", job.created_at);
                    if let Some(s) = &job.started_at {
                        println!("Started:  {s}");
                    }
                    if let Some(c) = &job.completed_at {
                        println!("Completed: {c}");
                    }
                    if let Some(e) = &job.error {
                        println!("Error:    {e}");
                    }
                    if let Some(cost) = job.cost_usd {
                        println!("Cost:     ${:.4}", cost);
                    }
                    if let Some(dur) = job.duration_ms {
                        println!("Duration: {}ms", dur);
                    }
                    println!("Prompt:   {}", truncate(&job.prompt, 100));
                }
                Err(e) => eprintln!("Error: {e}"),
            },
            None => match client.get_status().await {
                Ok(status) => {
                    println!("Queue Status");
                    println!("────────────────────");
                    println!(
                        "Pending:   {}",
                        status["queue"]["pending"].as_u64().unwrap_or(0)
                    );
                    println!(
                        "Running:   {}",
                        status["queue"]["running"].as_u64().unwrap_or(0)
                    );
                }
                Err(e) => eprintln!("Error: {e}"),
            },
        },
        Commands::Result { job_id } => match client.get_result(job_id).await {
            Ok(result) => {
                println!("{}", result.result);
                println!(
                    "\nCost: ${:.4} | Duration: {}ms",
                    result.cost_usd, result.duration_ms
                );
            }
            Err(e) => eprintln!("Error: {e}"),
        },
        Commands::List { status, limit } => match client.list_jobs(status.as_deref(), limit).await {
            Ok(resp) => {
                let items = resp["items"].as_array().unwrap_or(&Vec::new()).clone();
                if items.is_empty() {
                    println!("No jobs found");
                    return;
                }
                for item in &items {
                    let id = item["id"].as_str().unwrap_or("?");
                    let st = item["status"].as_str().unwrap_or("?");
                    let prompt = item["prompt"].as_str().unwrap_or("?");
                    println!(
                        "  {}  {:>9}  {}",
                        &id[..8.min(id.len())],
                        st,
                        truncate(prompt, 60)
                    );
                }
                println!("\n{} jobs", items.len());
            }
            Err(e) => eprintln!("Error: {e}"),
        },
        Commands::Logs { job_id } => match client.get_logs(job_id).await {
            Ok(resp) => {
                let lines = resp["lines"].as_array().unwrap_or(&Vec::new()).clone();
                if lines.is_empty() {
                    println!("No logs for job {job_id}");
                    return;
                }
                for line in &lines {
                    let text = line.as_str().unwrap_or("");
                    // Try to extract message type for nice display
                    if let Ok(val) = serde_json::from_str::<serde_json::Value>(text) {
                        let msg_type = val.get("type").and_then(|t| t.as_str()).unwrap_or("?");
                        match msg_type {
                            "assistant" => {
                                if let Some(msg) = val.get("message").and_then(|m| m.as_str()) {
                                    println!("  claude: {}", truncate(msg, 100));
                                }
                            }
                            "tool_use" => {
                                let tool = val.get("tool").and_then(|t| t.as_str()).unwrap_or("?");
                                println!("  > {tool}");
                            }
                            "result" => {
                                if let Some(r) = val.get("result").and_then(|r| r.as_str()) {
                                    println!("  result: {}", truncate(r, 100));
                                }
                            }
                            _ => {}
                        }
                    }
                }
                println!("\n{} log lines", lines.len());
            }
            Err(e) => eprintln!("Error: {e}"),
        },
        Commands::Cancel { job_id } => match client.cancel_job(job_id).await {
            Ok(_) => println!("Job {job_id} cancelled."),
            Err(e) => eprintln!("Error: {e}"),
        },
        Commands::Skill { command } => match command {
            SkillCommands::Create { id, name, r#type, content, file, description, tags } => {
                let content = match (content, file) {
                    (Some(c), _) => c,
                    (_, Some(f)) => std::fs::read_to_string(&f).unwrap_or_else(|e| {
                        eprintln!("Error reading file: {e}");
                        std::process::exit(1);
                    }),
                    _ => {
                        eprintln!("Either --content or --file must be specified");
                        std::process::exit(1);
                    }
                };
                let tags: Vec<String> = tags.split(',').map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).collect();
                match client.create_skill(&id, &name, &content, &description, &tags).await {
                    Ok(_) => println!("Skill '{id}' created."),
                    Err(e) => eprintln!("Error: {e}"),
                }
            }
            SkillCommands::List => match client.list_skills().await {
                Ok(resp) => {
                    let items = resp["items"].as_array().unwrap_or(&Vec::new()).clone();
                    if items.is_empty() {
                        println!("No skills");
                        return;
                    }
                    for item in &items {
                        let id = item["id"].as_str().unwrap_or("?");
                        let desc = item["description"].as_str().unwrap_or("");
                        let tags_arr = item["tags"].as_array();
                        let tags_str = tags_arr.map(|t| t.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>().join(", ")).unwrap_or_default();
                        let name = item["name"].as_str().unwrap_or("");
                        println!("  {:<20} {:<20} {}", id, name, if !tags_str.is_empty() { format!("[{}] {}", tags_str, desc) } else { desc.to_string() });
                    }
                }
                Err(e) => eprintln!("Error: {e}"),
            },
            SkillCommands::Show { id } => match client.get_skill(&id).await {
                Ok(skill) => {
                    println!("ID:          {}", skill["id"].as_str().unwrap_or("?"));
                    println!("Name:        {}", skill["name"].as_str().unwrap_or("?"));
                    println!("Files:       {}", skill["files"].as_object().map(|f| f.len()).unwrap_or(0));
                    println!("Description: {}", skill["description"].as_str().unwrap_or(""));
                    println!("");
                    println!("{}", skill["content"].as_str().unwrap_or(""));
                }
                Err(e) => eprintln!("Error: {e}"),
            },
            SkillCommands::Delete { id } => match client.delete_skill(&id).await {
                Ok(_) => println!("Skill '{id}' deleted."),
                Err(e) => eprintln!("Error: {e}"),
            },
        },
    }
}

fn truncate(s: &str, max: usize) -> &str {
    if s.len() <= max {
        s
    } else {
        &s[..max]
    }
}
