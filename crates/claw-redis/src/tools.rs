use chrono::Utc;
use claw_models::*;
use deadpool_redis::Pool;
use redis::AsyncCommands;

use crate::RedisError;

/// Create a new tool.
pub async fn create_tool(pool: &Pool, tool: &Tool) -> Result<(), RedisError> {
    let mut conn = pool.get().await?;
    let key = format!("claw:tool:{}", tool.id);
    let json = serde_json::to_string(tool)?;

    redis::pipe()
        .set(&key, &json)
        .sadd("claw:tools:index", &tool.id)
        .exec_async(&mut *conn)
        .await?;

    tracing::info!(tool_id = %tool.id, "Tool created");
    Ok(())
}

/// Get a tool by ID.
pub async fn get_tool(pool: &Pool, tool_id: &str) -> Result<Option<Tool>, RedisError> {
    let mut conn = pool.get().await?;
    let key = format!("claw:tool:{}", tool_id);
    let json: Option<String> = conn.get(&key).await?;
    match json {
        Some(j) => Ok(Some(serde_json::from_str(&j)?)),
        None => Ok(None),
    }
}

/// List all tools.
pub async fn list_tools(pool: &Pool) -> Result<Vec<Tool>, RedisError> {
    let mut conn = pool.get().await?;
    let ids: Vec<String> = redis::cmd("SMEMBERS")
        .arg("claw:tools:index")
        .query_async(&mut *conn)
        .await
        .unwrap_or_default();

    let mut tools = Vec::new();
    for id in &ids {
        let key = format!("claw:tool:{}", id);
        if let Ok(json) = conn.get::<_, String>(&key).await {
            if let Ok(tool) = serde_json::from_str::<Tool>(&json) {
                tools.push(tool);
            }
        }
    }
    tools.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(tools)
}

/// Update a tool.
pub async fn update_tool(pool: &Pool, tool: &Tool) -> Result<(), RedisError> {
    let mut conn = pool.get().await?;
    let key = format!("claw:tool:{}", tool.id);
    let json = serde_json::to_string(tool)?;
    let _: () = conn.set(&key, &json).await?;
    tracing::info!(tool_id = %tool.id, "Tool updated");
    Ok(())
}

/// Delete a tool.
pub async fn delete_tool(pool: &Pool, tool_id: &str) -> Result<(), RedisError> {
    let mut conn = pool.get().await?;
    redis::pipe()
        .del(format!("claw:tool:{}", tool_id))
        .srem("claw:tools:index", tool_id)
        .exec_async(&mut *conn)
        .await?;
    tracing::info!(tool_id, "Tool deleted");
    Ok(())
}

/// Resolve tools by explicit IDs (no tag matching — tools use explicit references only).
pub async fn resolve_tools(pool: &Pool, tool_ids: &[String]) -> Result<Vec<Tool>, RedisError> {
    if tool_ids.is_empty() {
        return Ok(Vec::new());
    }

    let mut conn = pool.get().await?;
    let mut resolved = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for id in tool_ids {
        if !seen.insert(id.clone()) {
            continue;
        }
        let key = format!("claw:tool:{}", id);
        match conn.get::<_, Option<String>>(&key).await {
            Ok(Some(json)) => {
                if let Ok(tool) = serde_json::from_str::<Tool>(&json) {
                    resolved.push(tool);
                }
            }
            _ => {
                tracing::warn!(tool_id = %id, "Referenced tool not found");
            }
        }
    }

    Ok(resolved)
}

/// Build a Tool helper for programmatic usage.
pub fn new_tool(
    id: &str,
    name: &str,
    install_commands: &str,
    check_command: &str,
) -> Tool {
    let now = Utc::now();
    Tool {
        id: id.to_string(),
        name: name.to_string(),
        description: String::new(),
        tags: Vec::new(),
        install_commands: install_commands.to_string(),
        check_command: check_command.to_string(),
        env_vars: Vec::new(),
        auth_script: None,
        skill_content: None,
        version: String::new(),
        author: String::new(),
        license: None,
        source_url: None,
        enabled: true,
        created_at: now,
        updated_at: now,
    }
}
