use chrono::Utc;
use claw_models::*;
use deadpool_redis::Pool;
use redis::AsyncCommands;

use crate::RedisError;

/// Create a new skill.
pub async fn create_skill(pool: &Pool, skill: &Skill) -> Result<(), RedisError> {
    let mut conn = pool.get().await?;
    let key = format!("claw:skill:{}", skill.id);
    let json = serde_json::to_string(skill)?;

    redis::pipe()
        .set(&key, &json)
        .sadd("claw:skills:index", &skill.id)
        .exec_async(&mut *conn)
        .await?;

    tracing::info!(skill_id = %skill.id, "Skill created");
    Ok(())
}

/// Get a skill by ID.
pub async fn get_skill(pool: &Pool, skill_id: &str) -> Result<Option<Skill>, RedisError> {
    let mut conn = pool.get().await?;
    let key = format!("claw:skill:{}", skill_id);
    let json: Option<String> = conn.get(&key).await?;
    match json {
        Some(j) => Ok(Some(serde_json::from_str(&j)?)),
        None => Ok(None),
    }
}

/// List all skills.
pub async fn list_skills(pool: &Pool) -> Result<Vec<Skill>, RedisError> {
    let mut conn = pool.get().await?;
    let ids: Vec<String> = redis::cmd("SMEMBERS")
        .arg("claw:skills:index")
        .query_async(&mut *conn)
        .await
        .unwrap_or_default();

    let mut skills = Vec::new();
    for id in &ids {
        let key = format!("claw:skill:{}", id);
        if let Ok(json) = conn.get::<_, String>(&key).await {
            if let Ok(skill) = serde_json::from_str::<Skill>(&json) {
                skills.push(skill);
            }
        }
    }
    skills.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(skills)
}

/// Update a skill.
pub async fn update_skill(pool: &Pool, skill: &Skill) -> Result<(), RedisError> {
    let mut conn = pool.get().await?;
    let key = format!("claw:skill:{}", skill.id);
    let json = serde_json::to_string(skill)?;
    let _: () = conn.set(&key, &json).await?;
    tracing::info!(skill_id = %skill.id, "Skill updated");
    Ok(())
}

/// Delete a skill.
pub async fn delete_skill(pool: &Pool, skill_id: &str) -> Result<(), RedisError> {
    let mut conn = pool.get().await?;
    redis::pipe()
        .del(format!("claw:skill:{}", skill_id))
        .srem("claw:skills:index", skill_id)
        .exec_async(&mut *conn)
        .await?;
    tracing::info!(skill_id, "Skill deleted");
    Ok(())
}

/// Resolve skills for a job: by explicit IDs + tag matching.
pub async fn resolve_skills(pool: &Pool, skill_ids: &[String], skill_tags: &[String]) -> Result<Vec<Skill>, RedisError> {
    let all_skills = list_skills(pool).await?;
    let mut resolved = Vec::new();
    let mut seen = std::collections::HashSet::new();

    // Explicit IDs first
    for id in skill_ids {
        if let Some(skill) = all_skills.iter().find(|s| &s.id == id) {
            if seen.insert(skill.id.clone()) {
                resolved.push(skill.clone());
            }
        } else {
            tracing::warn!(skill_id = %id, "Referenced skill not found");
        }
    }

    // Tag-matched skills
    if !skill_tags.is_empty() {
        for skill in &all_skills {
            if seen.contains(&skill.id) {
                continue;
            }
            let matches = skill.tags.iter().any(|t| skill_tags.contains(t));
            if matches {
                seen.insert(skill.id.clone());
                resolved.push(skill.clone());
            }
        }
    }

    Ok(resolved)
}

/// Build a CreateSkill helper for CLI/API usage.
pub fn new_skill(
    id: &str,
    name: &str,
    content: &str,
    description: &str,
    tags: Vec<String>,
    files: std::collections::HashMap<String, String>,
) -> Skill {
    let now = Utc::now();
    Skill {
        id: id.to_string(),
        name: name.to_string(),
        content: content.to_string(),
        description: description.to_string(),
        tags,
        files,
        created_at: now,
        updated_at: now,
    }
}
