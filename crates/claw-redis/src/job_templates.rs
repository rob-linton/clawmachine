use chrono::Utc;
use claw_models::*;
use deadpool_redis::Pool;
use redis::AsyncCommands;
use uuid::Uuid;

use crate::RedisError;

pub async fn create_job_template(pool: &Pool, req: &CreateJobTemplateRequest) -> Result<JobTemplate, RedisError> {
    let mut conn = pool.get().await?;
    let id = Uuid::new_v4();
    let now = Utc::now();

    let template = JobTemplate {
        id,
        name: req.name.clone(),
        description: req.description.clone(),
        prompt: req.prompt.clone(),
        skill_ids: req.skill_ids.clone(),
        tool_ids: req.tool_ids.clone(),
        workspace_id: req.workspace_id,
        model: req.model.clone(),
        timeout_secs: req.timeout_secs,
        allowed_tools: req.allowed_tools.clone(),
        output_dest: req.output_dest.clone(),
        tags: req.tags.clone(),
        priority: req.priority.unwrap_or(5),
        created_at: now,
        updated_at: now,
    };

    let json = serde_json::to_string(&template)?;
    redis::pipe()
        .set(format!("claw:template:{}", id), &json)
        .sadd("claw:templates:index", id.to_string())
        .exec_async(&mut *conn)
        .await?;

    tracing::info!(template_id = %id, name = %template.name, "Job template created");
    Ok(template)
}

pub async fn get_job_template(pool: &Pool, id: Uuid) -> Result<Option<JobTemplate>, RedisError> {
    let mut conn = pool.get().await?;
    let json: Option<String> = conn.get(format!("claw:template:{}", id)).await?;
    match json {
        Some(j) => Ok(Some(serde_json::from_str(&j)?)),
        None => Ok(None),
    }
}

pub async fn list_job_templates(pool: &Pool) -> Result<Vec<JobTemplate>, RedisError> {
    let mut conn = pool.get().await?;
    let ids: Vec<String> = redis::cmd("SMEMBERS")
        .arg("claw:templates:index")
        .query_async(&mut *conn)
        .await
        .unwrap_or_default();

    let mut templates = Vec::new();
    for id in &ids {
        let key = format!("claw:template:{}", id);
        if let Ok(json) = conn.get::<_, String>(&key).await {
            if let Ok(t) = serde_json::from_str::<JobTemplate>(&json) {
                templates.push(t);
            }
        }
    }
    templates.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(templates)
}

pub async fn update_job_template(pool: &Pool, template: &JobTemplate) -> Result<(), RedisError> {
    let mut conn = pool.get().await?;
    let mut t = template.clone();
    t.updated_at = Utc::now();
    let json = serde_json::to_string(&t)?;
    let _: () = conn.set(format!("claw:template:{}", t.id), &json).await?;
    Ok(())
}

pub async fn delete_job_template(pool: &Pool, id: Uuid) -> Result<(), RedisError> {
    let mut conn = pool.get().await?;

    // Check references from crons
    let crons = crate::list_crons(pool).await?;
    for cron in &crons {
        if cron.template_id == Some(id) {
            return Err(RedisError::Redis(redis::RedisError::from((
                redis::ErrorKind::ExtensionError,
                "Template is referenced by cron schedule",
                cron.id.to_string(),
            ))));
        }
    }

    // Check references from pipelines
    let pipelines = crate::list_pipelines(pool).await?;
    for pipeline in &pipelines {
        for step in &pipeline.steps {
            if step.template_id == Some(id) {
                return Err(RedisError::Redis(redis::RedisError::from((
                    redis::ErrorKind::ExtensionError,
                    "Template is referenced by pipeline",
                    pipeline.id.to_string(),
                ))));
            }
        }
    }

    redis::pipe()
        .del(format!("claw:template:{}", id))
        .srem("claw:templates:index", id.to_string())
        .exec_async(&mut *conn)
        .await?;

    tracing::info!(template_id = %id, "Job template deleted");
    Ok(())
}
