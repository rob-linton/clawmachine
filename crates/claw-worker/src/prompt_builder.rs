use claw_models::{Job, Skill, SkillType};
use deadpool_redis::Pool;

pub struct BuiltPrompt {
    pub prompt: String,
    pub skill_snapshot: serde_json::Value,
}

/// Resolve skills and build the final prompt with injections.
pub async fn build_prompt(pool: &Pool, job: &Job) -> BuiltPrompt {
    let skills = claw_redis::resolve_skills(pool, &job.skill_ids, &job.skill_tags)
        .await
        .unwrap_or_default();

    let mut sections: Vec<String> = Vec::new();

    // Inject template skills wrapped in <skill> tags
    let templates: Vec<&Skill> = skills.iter().filter(|s| s.skill_type == SkillType::Template).collect();
    for skill in &templates {
        sections.push(format!(
            "<skill name=\"{}\">\n{}\n</skill>",
            skill.id, skill.content
        ));
    }

    // Mention available scripts
    let scripts: Vec<&Skill> = skills.iter().filter(|s| s.skill_type == SkillType::Script).collect();
    if !scripts.is_empty() {
        let names: Vec<String> = scripts.iter().map(|s| s.id.clone()).collect();
        sections.push(format!(
            "You have access to the following scripts: {}. Run them if needed for your task.",
            names.join(", ")
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

    // Warn if very large
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
            "type": s.skill_type.to_string(),
            "content_len": s.content.len(),
        })
    }).collect();

    BuiltPrompt {
        prompt,
        skill_snapshot: serde_json::json!(snapshot),
    }
}
