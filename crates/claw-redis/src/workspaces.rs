use chrono::Utc;
use claw_models::*;
use deadpool_redis::Pool;
use redis::AsyncCommands;
use uuid::Uuid;

use crate::RedisError;

pub async fn create_workspace(pool: &Pool, req: &CreateWorkspaceRequest) -> Result<Workspace, RedisError> {
    let mut conn = pool.get().await?;
    let id = Uuid::new_v4();
    let now = Utc::now();

    // Only set path for legacy workspaces (when path is explicitly provided)
    let path = if req.path.is_some() {
        Some(req.path.clone().unwrap_or_else(|| {
            let base = dirs::home_dir()
                .unwrap_or_else(|| "/tmp".into())
                .join(".claw")
                .join("workspaces");
            base.join(id.to_string())
        }))
    } else {
        None // New-style workspace — bare repo path derived from ID
    };

    let workspace = Workspace {
        id,
        name: req.name.clone(),
        description: req.description.clone().unwrap_or_default(),
        path,
        skill_ids: req.skill_ids.clone(),
        claude_md: req.claude_md.clone(),
        persistence: req.persistence.clone().unwrap_or_default(),
        remote_url: req.remote_url.clone(),
        base_image: req.base_image.clone(),
        memory_limit: req.memory_limit.clone(),
        cpu_limit: req.cpu_limit,
        network_mode: req.network_mode.clone(),
        parent_workspace_id: req.parent_workspace_id,
        parent_ref: req.parent_ref.clone(),
        created_at: now,
        updated_at: now,
    };

    let json = serde_json::to_string(&workspace)?;
    let key = format!("claw:workspace:{}", id);

    redis::pipe()
        .set(&key, &json)
        .sadd("claw:workspaces:index", id.to_string())
        .exec_async(&mut *conn)
        .await?;

    tracing::info!(workspace_id = %id, name = %workspace.name, "Workspace created");
    Ok(workspace)
}

pub async fn get_workspace(pool: &Pool, id: Uuid) -> Result<Option<Workspace>, RedisError> {
    let mut conn = pool.get().await?;
    let key = format!("claw:workspace:{}", id);
    let json: Option<String> = conn.get(&key).await?;
    match json {
        Some(j) => Ok(Some(serde_json::from_str(&j)?)),
        None => Ok(None),
    }
}

pub async fn list_workspaces(pool: &Pool) -> Result<Vec<Workspace>, RedisError> {
    let mut conn = pool.get().await?;
    let ids: Vec<String> = redis::cmd("SMEMBERS")
        .arg("claw:workspaces:index")
        .query_async(&mut *conn)
        .await
        .unwrap_or_default();

    let mut workspaces = Vec::new();
    for id in &ids {
        let key = format!("claw:workspace:{}", id);
        if let Ok(json) = conn.get::<_, String>(&key).await {
            if let Ok(ws) = serde_json::from_str::<Workspace>(&json) {
                workspaces.push(ws);
            }
        }
    }

    workspaces.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(workspaces)
}

pub async fn update_workspace(pool: &Pool, workspace: &Workspace) -> Result<(), RedisError> {
    let mut conn = pool.get().await?;
    let mut ws = workspace.clone();
    ws.updated_at = Utc::now();
    let json = serde_json::to_string(&ws)?;
    let key = format!("claw:workspace:{}", ws.id);
    let _: () = conn.set(&key, &json).await?;
    tracing::info!(workspace_id = %ws.id, "Workspace updated");
    Ok(())
}

pub async fn delete_workspace(pool: &Pool, id: Uuid) -> Result<(), RedisError> {
    let mut conn = pool.get().await?;

    // Check if any cron schedules reference this workspace
    let crons = crate::list_crons(pool).await?;
    for cron in &crons {
        if cron.workspace_id == Some(id) {
            return Err(RedisError::Redis(redis::RedisError::from((
                redis::ErrorKind::ExtensionError,
                "Workspace is referenced by cron schedule",
                cron.id.to_string(),
            ))));
        }
    }

    // Check if any job templates reference this workspace
    let templates = crate::list_job_templates(pool).await?;
    for tmpl in &templates {
        if tmpl.workspace_id == Some(id) {
            return Err(RedisError::Redis(redis::RedisError::from((
                redis::ErrorKind::ExtensionError,
                "Workspace is referenced by job template",
                tmpl.id.to_string(),
            ))));
        }
    }

    // Check if any pipelines reference this workspace
    let pipelines = crate::list_pipelines(pool).await?;
    for pipeline in &pipelines {
        if pipeline.workspace_id == Some(id) {
            return Err(RedisError::Redis(redis::RedisError::from((
                redis::ErrorKind::ExtensionError,
                "Workspace is referenced by pipeline",
                pipeline.id.to_string(),
            ))));
        }
    }

    // Check if any running/pending jobs are using this workspace
    let jobs = crate::list_jobs(pool, None, 100, Some(id)).await?;
    for job in &jobs {
        if job.status == claw_models::JobStatus::Running || job.status == claw_models::JobStatus::Pending {
            return Err(RedisError::Redis(redis::RedisError::from((
                redis::ErrorKind::ExtensionError,
                "Workspace has active jobs (pending or running)",
                job.id.to_string(),
            ))));
        }
    }

    // Get workspace to check parent (for removing from parent's children set)
    let ws = get_workspace(pool, id).await?.unwrap_or_else(|| {
        Workspace {
            id,
            name: String::new(),
            description: String::new(),
            path: None,
            skill_ids: vec![],
            claude_md: None,
            persistence: WorkspacePersistence::default(),
            remote_url: None,
            base_image: None,
            memory_limit: None,
            cpu_limit: None,
            network_mode: None,
            parent_workspace_id: None,
            parent_ref: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    });

    // Remove from parent's children set if this workspace was forked
    if let Some(parent_id) = ws.parent_workspace_id {
        let children_key = format!("claw:workspace:{}:children", parent_id);
        let _: () = conn.srem(&children_key, id.to_string()).await.unwrap_or(());
    }

    redis::pipe()
        .del(format!("claw:workspace:{}", id))
        .del(format!("claw:workspace:{}:events", id))
        .del(format!("claw:workspace:{}:children", id))
        .srem("claw:workspaces:index", id.to_string())
        .exec_async(&mut *conn)
        .await?;

    tracing::info!(workspace_id = %id, "Workspace deleted");
    Ok(())
}

/// Acquire an exclusive lock on a workspace. Returns true if lock acquired.
pub async fn acquire_workspace_lock(pool: &Pool, workspace_id: Uuid, job_id: Uuid, ttl_secs: u64) -> Result<bool, RedisError> {
    let mut conn = pool.get().await?;
    let key = format!("claw:workspace:{}:lock", workspace_id);
    let value = job_id.to_string();

    // SETNX with TTL via Lua for atomicity
    let script = redis::Script::new(
        r#"
        local ok = redis.call('SET', KEYS[1], ARGV[1], 'NX', 'EX', ARGV[2])
        if ok then return 1 else return 0 end
        "#,
    );
    let result: i32 = script
        .key(&key)
        .arg(&value)
        .arg(ttl_secs)
        .invoke_async(&mut *conn)
        .await?;

    Ok(result == 1)
}

/// Release workspace lock, but only if we own it (CAS via Lua).
pub async fn release_workspace_lock(pool: &Pool, workspace_id: Uuid, job_id: Uuid) -> Result<(), RedisError> {
    let mut conn = pool.get().await?;
    let key = format!("claw:workspace:{}:lock", workspace_id);
    let value = job_id.to_string();

    let script = redis::Script::new(
        r#"
        if redis.call('GET', KEYS[1]) == ARGV[1] then
            return redis.call('DEL', KEYS[1])
        else
            return 0
        end
        "#,
    );
    let _: i32 = script
        .key(&key)
        .arg(&value)
        .invoke_async(&mut *conn)
        .await?;

    Ok(())
}

// --- Workspace event log ---

/// Append an event to a workspace's event timeline. Capped at 1000 events.
pub async fn append_workspace_event(pool: &Pool, workspace_id: Uuid, event: &WorkspaceEvent) -> Result<(), RedisError> {
    let mut conn = pool.get().await?;
    let key = format!("claw:workspace:{}:events", workspace_id);
    let json = serde_json::to_string(event)?;

    redis::pipe()
        .lpush(&key, &json)
        .ltrim(&key, 0, 999) // Keep newest 1000
        .exec_async(&mut *conn)
        .await?;
    Ok(())
}

/// List workspace events (newest first) with pagination.
pub async fn list_workspace_events(pool: &Pool, workspace_id: Uuid, limit: usize, offset: usize) -> Result<(Vec<WorkspaceEvent>, usize), RedisError> {
    let mut conn = pool.get().await?;
    let key = format!("claw:workspace:{}:events", workspace_id);

    let total: usize = redis::cmd("LLEN").arg(&key).query_async(&mut *conn).await.unwrap_or(0);
    let end = offset + limit;
    let jsons: Vec<String> = redis::cmd("LRANGE")
        .arg(&key)
        .arg(offset)
        .arg(end.saturating_sub(1))
        .query_async(&mut *conn)
        .await
        .unwrap_or_default();

    let events: Vec<WorkspaceEvent> = jsons
        .iter()
        .filter_map(|j| serde_json::from_str(j).ok())
        .collect();

    Ok((events, total))
}

// --- Workspace children index ---

/// Add a child workspace to a parent's children set.
pub async fn add_child_workspace(pool: &Pool, parent_id: Uuid, child_id: Uuid) -> Result<(), RedisError> {
    let mut conn = pool.get().await?;
    let key = format!("claw:workspace:{}:children", parent_id);
    let _: () = conn.sadd(&key, child_id.to_string()).await?;
    Ok(())
}

/// Remove a child workspace from a parent's children set.
pub async fn remove_child_workspace(pool: &Pool, parent_id: Uuid, child_id: Uuid) -> Result<(), RedisError> {
    let mut conn = pool.get().await?;
    let key = format!("claw:workspace:{}:children", parent_id);
    let _: () = conn.srem(&key, child_id.to_string()).await?;
    Ok(())
}

/// Count direct children of a workspace.
pub async fn count_children(pool: &Pool, workspace_id: Uuid) -> Result<u32, RedisError> {
    let mut conn = pool.get().await?;
    let key = format!("claw:workspace:{}:children", workspace_id);
    let count: u32 = redis::cmd("SCARD").arg(&key).query_async(&mut *conn).await.unwrap_or(0);
    Ok(count)
}

/// List child workspace IDs.
pub async fn list_child_workspaces(pool: &Pool, workspace_id: Uuid) -> Result<Vec<String>, RedisError> {
    let mut conn = pool.get().await?;
    let key = format!("claw:workspace:{}:children", workspace_id);
    let ids: Vec<String> = redis::cmd("SMEMBERS").arg(&key).query_async(&mut *conn).await.unwrap_or_default();
    Ok(ids)
}

/// Re-queue a job back to pending (used when workspace is locked).
/// Does NOT increment retry_count — that's reserved for execution failure retries.
pub async fn requeue_job(pool: &Pool, job_id: Uuid, priority: u8) -> Result<(), RedisError> {
    let mut conn = pool.get().await?;
    let job_key = format!("claw:job:{}", job_id);

    let job_json: Option<String> = conn.get(&job_key).await?;
    let Some(json) = job_json else {
        return Err(RedisError::NotFound(job_id));
    };
    let mut job: claw_models::Job = serde_json::from_str(&json)?;
    job.status = claw_models::JobStatus::Pending;
    job.worker_id = None;
    job.started_at = None;

    let updated_json = serde_json::to_string(&job)?;
    let queue_key = format!("claw:queue:pending:{}", priority.min(9));

    redis::pipe()
        .set(&job_key, &updated_json)
        .srem("claw:queue:running", job_id.to_string())
        .rpush(&queue_key, job_id.to_string())
        .exec_async(&mut *conn)
        .await?;

    tracing::info!(job_id = %job_id, "Job re-queued (workspace locked)");
    Ok(())
}
