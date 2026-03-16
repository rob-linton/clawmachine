use claw_models::{Job, Skill, SkillType};

pub struct BuiltPrompt {
    pub prompt: String,
    pub skill_snapshot: serde_json::Value,
}

/// Build the prompt with only template skills injected.
/// ClaudeConfig and Script skills are handled by the environment module (disk-based).
pub fn build_prompt(job: &Job, skills: &[Skill]) -> BuiltPrompt {
    let mut sections: Vec<String> = Vec::new();

    // Only inject template skills wrapped in <skill> tags
    let templates: Vec<&Skill> = skills.iter().filter(|s| s.skill_type == SkillType::Template).collect();
    for skill in &templates {
        sections.push(format!(
            "<skill name=\"{}\">\n{}\n</skill>",
            skill.id, skill.content
        ));
    }

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

    // Skill snapshot records ALL skills with injection method
    let snapshot: Vec<serde_json::Value> = skills.iter().map(|s| {
        let injection = match s.skill_type {
            SkillType::Template => "prompt",
            SkillType::ClaudeConfig => "claude_md",
            SkillType::Script => "disk",
        };
        serde_json::json!({
            "id": s.id,
            "type": s.skill_type.to_string(),
            "content_len": s.content.len(),
            "files_count": s.files.len(),
            "injection": injection,
        })
    }).collect();

    BuiltPrompt {
        prompt,
        skill_snapshot: serde_json::json!(snapshot),
    }
}
