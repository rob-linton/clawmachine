use chrono::Utc;
use claw_models::*;
use deadpool_redis::Pool;
use redis::AsyncCommands;
use uuid::Uuid;

use crate::RedisError;

pub async fn create_pipeline(pool: &Pool, req: &CreatePipelineRequest) -> Result<Pipeline, RedisError> {
    let mut conn = pool.get().await?;
    let id = Uuid::new_v4();
    let now = Utc::now();

    let pipeline = Pipeline {
        id,
        name: req.name.clone(),
        description: req.description.clone(),
        workspace_id: req.workspace_id,
        steps: req.steps.clone(),
        created_at: now,
    };

    let json = serde_json::to_string(&pipeline)?;
    redis::pipe()
        .set(format!("claw:pipeline:{}", id), &json)
        .sadd("claw:pipelines:index", id.to_string())
        .exec_async(&mut *conn)
        .await?;

    tracing::info!(pipeline_id = %id, name = %pipeline.name, steps = pipeline.steps.len(), "Pipeline created");
    Ok(pipeline)
}

pub async fn get_pipeline(pool: &Pool, id: Uuid) -> Result<Option<Pipeline>, RedisError> {
    let mut conn = pool.get().await?;
    let json: Option<String> = conn.get(format!("claw:pipeline:{}", id)).await?;
    match json {
        Some(j) => Ok(Some(serde_json::from_str(&j)?)),
        None => Ok(None),
    }
}

pub async fn list_pipelines(pool: &Pool) -> Result<Vec<Pipeline>, RedisError> {
    let mut conn = pool.get().await?;
    let ids: Vec<String> = redis::cmd("SMEMBERS")
        .arg("claw:pipelines:index")
        .query_async(&mut *conn)
        .await
        .unwrap_or_default();

    let mut pipelines = Vec::new();
    for id in &ids {
        let key = format!("claw:pipeline:{}", id);
        if let Ok(json) = conn.get::<_, String>(&key).await {
            if let Ok(p) = serde_json::from_str::<Pipeline>(&json) {
                pipelines.push(p);
            }
        }
    }
    pipelines.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    Ok(pipelines)
}

pub async fn update_pipeline(pool: &Pool, pipeline: &Pipeline) -> Result<(), RedisError> {
    let mut conn = pool.get().await?;
    let json = serde_json::to_string(pipeline)?;
    let _: () = conn.set(format!("claw:pipeline:{}", pipeline.id), &json).await?;
    tracing::info!(pipeline_id = %pipeline.id, name = %pipeline.name, "Pipeline updated");
    Ok(())
}

pub async fn delete_pipeline(pool: &Pool, id: Uuid) -> Result<(), RedisError> {
    let mut conn = pool.get().await?;
    redis::pipe()
        .del(format!("claw:pipeline:{}", id))
        .srem("claw:pipelines:index", id.to_string())
        .exec_async(&mut *conn)
        .await?;
    Ok(())
}

// --- Pipeline Runs ---

pub async fn create_pipeline_run(pool: &Pool, pipeline: &Pipeline) -> Result<PipelineRun, RedisError> {
    let mut conn = pool.get().await?;
    let id = Uuid::new_v4();
    let now = Utc::now();

    let run = PipelineRun {
        id,
        pipeline_id: pipeline.id,
        pipeline_name: pipeline.name.clone(),
        workspace_id: pipeline.workspace_id,
        status: PipelineStatus::Running,
        step_jobs: vec![None; pipeline.steps.len()],
        current_step: 0,
        created_at: now,
        completed_at: None,
        error: None,
    };

    let json = serde_json::to_string(&run)?;
    redis::pipe()
        .set(format!("claw:pipeline-run:{}", id), &json)
        .sadd("claw:pipeline-runs:index", id.to_string())
        .exec_async(&mut *conn)
        .await?;

    tracing::info!(run_id = %id, pipeline_id = %pipeline.id, "Pipeline run created");
    Ok(run)
}

pub async fn get_pipeline_run(pool: &Pool, id: Uuid) -> Result<Option<PipelineRun>, RedisError> {
    let mut conn = pool.get().await?;
    let json: Option<String> = conn.get(format!("claw:pipeline-run:{}", id)).await?;
    match json {
        Some(j) => Ok(Some(serde_json::from_str(&j)?)),
        None => Ok(None),
    }
}

pub async fn update_pipeline_run(pool: &Pool, run: &PipelineRun) -> Result<(), RedisError> {
    let mut conn = pool.get().await?;
    let json = serde_json::to_string(run)?;
    let _: () = conn.set(format!("claw:pipeline-run:{}", run.id), &json).await?;
    Ok(())
}

pub async fn list_pipeline_runs(pool: &Pool, pipeline_id: Option<Uuid>) -> Result<Vec<PipelineRun>, RedisError> {
    let mut conn = pool.get().await?;
    let ids: Vec<String> = redis::cmd("SMEMBERS")
        .arg("claw:pipeline-runs:index")
        .query_async(&mut *conn)
        .await
        .unwrap_or_default();

    let mut runs = Vec::new();
    for id in &ids {
        let key = format!("claw:pipeline-run:{}", id);
        if let Ok(json) = conn.get::<_, String>(&key).await {
            if let Ok(run) = serde_json::from_str::<PipelineRun>(&json) {
                if let Some(pid) = pipeline_id {
                    if run.pipeline_id != pid { continue; }
                }
                runs.push(run);
            }
        }
    }
    runs.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    Ok(runs)
}
