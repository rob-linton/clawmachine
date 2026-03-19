use chrono::Utc;
use claw_models::*;
use deadpool_redis::Pool;
use redis::AsyncCommands;
use uuid::Uuid;

use crate::RedisError;

pub async fn create_cron(pool: &Pool, req: &CreateCronRequest) -> Result<CronSchedule, RedisError> {
    let mut conn = pool.get().await?;
    let id = Uuid::new_v4();
    let now = Utc::now();

    let cron = CronSchedule {
        id,
        name: req.name.clone(),
        schedule: req.schedule.clone(),
        enabled: req.enabled,
        prompt: req.prompt.clone(),
        skill_ids: req.skill_ids.clone(),
        tool_ids: req.tool_ids.clone(),
        working_dir: req.working_dir.clone().unwrap_or_else(|| ".".into()),
        model: req.model.clone(),
        max_budget_usd: req.max_budget_usd,
        output_dest: req.output_dest.clone(),
        tags: req.tags.clone(),
        priority: req.priority.unwrap_or(5),
        workspace_id: req.workspace_id,
        template_id: req.template_id,
        last_run: None,
        last_job_id: None,
        created_at: now,
    };

    let json = serde_json::to_string(&cron)?;
    let key = format!("claw:cron:{}", id);

    redis::pipe()
        .set(&key, &json)
        .sadd("claw:crons:index", id.to_string())
        .exec_async(&mut *conn)
        .await?;

    tracing::info!(cron_id = %id, name = %cron.name, "Cron created");
    Ok(cron)
}

pub async fn get_cron(pool: &Pool, cron_id: Uuid) -> Result<Option<CronSchedule>, RedisError> {
    let mut conn = pool.get().await?;
    let key = format!("claw:cron:{}", cron_id);
    let json: Option<String> = conn.get(&key).await?;
    match json {
        Some(j) => Ok(Some(serde_json::from_str(&j)?)),
        None => Ok(None),
    }
}

pub async fn list_crons(pool: &Pool) -> Result<Vec<CronSchedule>, RedisError> {
    let mut conn = pool.get().await?;
    let ids: Vec<String> = redis::cmd("SMEMBERS")
        .arg("claw:crons:index")
        .query_async(&mut *conn)
        .await
        .unwrap_or_default();

    let mut crons = Vec::new();
    for id in &ids {
        let key = format!("claw:cron:{}", id);
        if let Ok(json) = conn.get::<_, String>(&key).await {
            if let Ok(c) = serde_json::from_str::<CronSchedule>(&json) {
                crons.push(c);
            }
        }
    }
    crons.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(crons)
}

pub async fn update_cron(pool: &Pool, cron: &CronSchedule) -> Result<(), RedisError> {
    let mut conn = pool.get().await?;
    let key = format!("claw:cron:{}", cron.id);
    let json = serde_json::to_string(cron)?;
    let _: () = conn.set(&key, &json).await?;
    Ok(())
}

pub async fn delete_cron(pool: &Pool, cron_id: Uuid) -> Result<(), RedisError> {
    let mut conn = pool.get().await?;
    redis::pipe()
        .del(format!("claw:cron:{}", cron_id))
        .srem("claw:crons:index", cron_id.to_string())
        .exec_async(&mut *conn)
        .await?;
    tracing::info!(cron_id = %cron_id, "Cron deleted");
    Ok(())
}

/// Record that a cron just fired: atomically update only last_run and last_job_id
/// without overwriting other fields (prevents race with concurrent PUT updates).
pub async fn record_cron_fire(pool: &Pool, cron_id: Uuid, job_id: Uuid) -> Result<(), RedisError> {
    let mut conn = pool.get().await?;
    let key = format!("claw:cron:{}", cron_id);
    let now = Utc::now().to_rfc3339();

    // Lua script: atomically read-modify-write only last_run and last_job_id
    let script = redis::Script::new(
        r#"
        local json = redis.call('GET', KEYS[1])
        if not json then return 0 end
        local cron = cjson.decode(json)
        cron['last_run'] = ARGV[1]
        cron['last_job_id'] = ARGV[2]
        redis.call('SET', KEYS[1], cjson.encode(cron))
        return 1
        "#,
    );

    let _: i32 = script
        .key(&key)
        .arg(&now)
        .arg(job_id.to_string())
        .invoke_async(&mut *conn)
        .await?;

    Ok(())
}
