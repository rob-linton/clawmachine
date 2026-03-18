use claw_models::{Job, Skill, Workspace};

pub struct BuiltPrompt {
    /// The user's prompt — passed through unmodified.
    pub prompt: String,
    /// System prompt appendix — metadata + completion instruction.
    /// Passed via --append-system-prompt to keep it out of the user's prompt.
    pub system_prompt: String,
    /// Skill snapshot for reproducibility.
    pub skill_snapshot: serde_json::Value,
}

/// Build the prompt. The user's prompt is passed through unmodified.
/// Metadata and instructions go into a separate system prompt appendix.
pub fn build_prompt(job: &Job, skills: &[Skill], workspace: Option<&Workspace>) -> BuiltPrompt {
    // User prompt passes through exactly as written
    let prompt = job.prompt.clone();

    // System prompt appendix — orchestration context
    let mut system_parts: Vec<String> = Vec::new();

    // Workspace context — helps Claude understand what the working directory is for
    // and prevents it from wasting turns exploring an empty workspace.
    if let Some(ws) = workspace {
        let ws_desc = if ws.description.is_empty() {
            format!("Workspace: {}.", ws.name)
        } else {
            format!("Workspace: {} — {}.", ws.name, ws.description)
        };
        system_parts.push(ws_desc);
        system_parts.push(
            "The working directory is a git-managed workspace. \
             If the task does not require analyzing existing files, \
             proceed directly to the task without exploring the workspace first."
                .to_string(),
        );
    }

    if !skills.is_empty() {
        let skill_ids: Vec<&str> = skills.iter().map(|s| s.id.as_str()).collect();
        system_parts.push(format!(
            "Skills deployed to .claude/skills/: {}.",
            skill_ids.join(", ")
        ));
    }

    system_parts.push(format!("Job ID: {}. Source: {}.", job.id, job.source));

    system_parts.push(
        "When you have completed the task, end with a final summary of what you did, \
         what you found or concluded, and any files you created or modified."
            .to_string(),
    );

    let system_prompt = system_parts.join(" ");

    if prompt.len() > 100_000 {
        tracing::warn!(
            job_id = %job.id,
            prompt_len = prompt.len(),
            "Prompt exceeds 100K characters"
        );
    }

    // Skill snapshot for reproducibility
    let snapshot: Vec<serde_json::Value> = skills
        .iter()
        .map(|s| {
            serde_json::json!({
                "id": s.id,
                "content_len": s.content.len(),
                "files_count": s.files.len(),
            })
        })
        .collect();

    BuiltPrompt {
        prompt,
        system_prompt,
        skill_snapshot: serde_json::json!(snapshot),
    }
}
