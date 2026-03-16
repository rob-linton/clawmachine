use claw_models::{Job, Skill};

pub struct BuiltPrompt {
    pub prompt: String,
    pub skill_snapshot: serde_json::Value,
}

/// Build the prompt. Skills are deployed to disk by environment.rs,
/// so the prompt only contains context metadata + the user's prompt.
pub fn build_prompt(job: &Job, skills: &[Skill]) -> BuiltPrompt {
    let mut sections: Vec<String> = Vec::new();

    // Context metadata
    sections.push(format!(
        "[Job ID: {}] [Source: {}]",
        job.id, job.source
    ));

    // The actual user prompt
    sections.push(job.prompt.clone());

    let prompt = sections.join("\n\n");

    if prompt.len() > 100_000 {
        tracing::warn!(
            job_id = %job.id,
            prompt_len = prompt.len(),
            "Assembled prompt exceeds 100K characters"
        );
    }

    // Skill snapshot for reproducibility
    let snapshot: Vec<serde_json::Value> = skills.iter().map(|s| {
        serde_json::json!({
            "id": s.id,
            "content_len": s.content.len(),
            "files_count": s.files.len(),
        })
    }).collect();

    BuiltPrompt {
        prompt,
        skill_snapshot: serde_json::json!(snapshot),
    }
}
