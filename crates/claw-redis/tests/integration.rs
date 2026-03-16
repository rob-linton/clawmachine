use claw_models::*;
use claw_redis::*;

fn test_pool() -> deadpool_redis::Pool {
    let url = std::env::var("CLAW_REDIS_URL")
        .unwrap_or_else(|_| "redis://127.0.0.1:6379/15".into()); // Use DB 15 for tests
    create_pool(&url)
}

async fn flush_test_db(pool: &deadpool_redis::Pool) {
    let mut conn = pool.get().await.unwrap();
    let _: () = deadpool_redis::redis::cmd("FLUSHDB")
        .query_async(&mut *conn)
        .await
        .unwrap();
}

#[tokio::test]
async fn submit_and_get_job() {
    dotenvy::dotenv().ok();
    let pool = test_pool();
    flush_test_db(&pool).await;

    let req = CreateJobRequest {
        prompt: "test prompt".into(),
        skill_ids: vec![],
        skill_tags: vec![],
        working_dir: None,
        model: Some("sonnet".into()),
        max_budget_usd: None,
        allowed_tools: None,
        output_dest: OutputDest::Redis,
        tags: vec!["test".into()],
        priority: Some(7),
        timeout_secs: None, workspace_id: None,
    };

    let job = submit_job(&pool, &req, JobSource::Api).await.unwrap();
    assert_eq!(job.status, JobStatus::Pending);
    assert_eq!(job.prompt, "test prompt");
    assert_eq!(job.priority, 7);
    assert_eq!(job.tags, vec!["test"]);

    // Get it back
    let fetched = get_job(&pool, job.id).await.unwrap();
    assert_eq!(fetched.id, job.id);
    assert_eq!(fetched.prompt, "test prompt");
}

#[tokio::test]
async fn claim_job_returns_highest_priority() {
    dotenvy::dotenv().ok();
    let pool = test_pool();
    flush_test_db(&pool).await;

    // Submit low priority
    let low = CreateJobRequest {
        prompt: "low priority".into(),
        skill_ids: vec![], skill_tags: vec![], working_dir: None,
        model: None, max_budget_usd: None, allowed_tools: None,
        output_dest: OutputDest::Redis, tags: vec![], priority: Some(1), timeout_secs: None, workspace_id: None,
    };
    submit_job(&pool, &low, JobSource::Api).await.unwrap();

    // Submit high priority
    let high = CreateJobRequest {
        prompt: "high priority".into(),
        priority: Some(9),
        ..low.clone()
    };
    submit_job(&pool, &high, JobSource::Api).await.unwrap();

    // Claim should return the high priority job first
    let claimed = claim_job(&pool, "test-worker").await.unwrap().unwrap();
    assert_eq!(claimed.prompt, "high priority");
    assert_eq!(claimed.status, JobStatus::Running);
}

#[tokio::test]
async fn complete_and_get_result() {
    dotenvy::dotenv().ok();
    let pool = test_pool();
    flush_test_db(&pool).await;

    let req = CreateJobRequest {
        prompt: "math".into(),
        skill_ids: vec![], skill_tags: vec![], working_dir: None,
        model: None, max_budget_usd: None, allowed_tools: None,
        output_dest: OutputDest::Redis, tags: vec![], priority: None, timeout_secs: None, workspace_id: None,
    };
    let job = submit_job(&pool, &req, JobSource::Cli).await.unwrap();
    claim_job(&pool, "w1").await.unwrap();

    complete_job(&pool, job.id, "42", 0.05, 1234).await.unwrap();

    let result = get_result(&pool, job.id).await.unwrap();
    assert_eq!(result.result, "42");
    assert_eq!(result.cost_usd, 0.05);
    assert_eq!(result.duration_ms, 1234);

    let updated = get_job(&pool, job.id).await.unwrap();
    assert_eq!(updated.status, JobStatus::Completed);
}

#[tokio::test]
async fn fail_job_stores_error() {
    dotenvy::dotenv().ok();
    let pool = test_pool();
    flush_test_db(&pool).await;

    let req = CreateJobRequest {
        prompt: "will fail".into(),
        skill_ids: vec![], skill_tags: vec![], working_dir: None,
        model: None, max_budget_usd: None, allowed_tools: None,
        output_dest: OutputDest::Redis, tags: vec![], priority: None, timeout_secs: None, workspace_id: None,
    };
    let job = submit_job(&pool, &req, JobSource::Api).await.unwrap();
    claim_job(&pool, "w1").await.unwrap();

    fail_job(&pool, job.id, "something went wrong").await.unwrap();

    let updated = get_job(&pool, job.id).await.unwrap();
    assert_eq!(updated.status, JobStatus::Failed);
    assert_eq!(updated.error.as_deref(), Some("something went wrong"));
}

#[tokio::test]
async fn cancel_pending_job() {
    dotenvy::dotenv().ok();
    let pool = test_pool();
    flush_test_db(&pool).await;

    let req = CreateJobRequest {
        prompt: "cancel me".into(),
        skill_ids: vec![], skill_tags: vec![], working_dir: None,
        model: None, max_budget_usd: None, allowed_tools: None,
        output_dest: OutputDest::Redis, tags: vec![], priority: None, timeout_secs: None, workspace_id: None,
    };
    let job = submit_job(&pool, &req, JobSource::Api).await.unwrap();

    cancel_job(&pool, job.id).await.unwrap();

    let updated = get_job(&pool, job.id).await.unwrap();
    assert_eq!(updated.status, JobStatus::Cancelled);
}

#[tokio::test]
async fn list_jobs_returns_all() {
    dotenvy::dotenv().ok();
    let pool = test_pool();
    flush_test_db(&pool).await;

    let req = CreateJobRequest {
        prompt: "job 1".into(),
        skill_ids: vec![], skill_tags: vec![], working_dir: None,
        model: None, max_budget_usd: None, allowed_tools: None,
        output_dest: OutputDest::Redis, tags: vec![], priority: None, timeout_secs: None, workspace_id: None,
    };
    submit_job(&pool, &req, JobSource::Api).await.unwrap();

    let req2 = CreateJobRequest { prompt: "job 2".into(), ..req.clone() };
    submit_job(&pool, &req2, JobSource::Api).await.unwrap();

    let jobs = list_jobs(&pool, None, 50, None).await.unwrap();
    assert_eq!(jobs.len(), 2);
}

#[tokio::test]
async fn delete_job_removes_all_data() {
    dotenvy::dotenv().ok();
    let pool = test_pool();
    flush_test_db(&pool).await;

    let req = CreateJobRequest {
        prompt: "delete me".into(),
        skill_ids: vec![], skill_tags: vec![], working_dir: None,
        model: None, max_budget_usd: None, allowed_tools: None,
        output_dest: OutputDest::Redis, tags: vec![], priority: None, timeout_secs: None, workspace_id: None,
    };
    let job = submit_job(&pool, &req, JobSource::Api).await.unwrap();
    cancel_job(&pool, job.id).await.unwrap();

    delete_job(&pool, job.id).await.unwrap();

    let result = get_job(&pool, job.id).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn skill_crud() {
    dotenvy::dotenv().ok();
    let pool = test_pool();
    flush_test_db(&pool).await;

    let skill = new_skill("test-skill", "Test", "content here", "desc", vec!["tag1".into()], Default::default());
    create_skill(&pool, &skill).await.unwrap();

    let fetched = get_skill(&pool, "test-skill").await.unwrap().unwrap();
    assert_eq!(fetched.name, "Test");
    assert_eq!(fetched.content, "content here");

    let all = list_skills(&pool).await.unwrap();
    assert_eq!(all.len(), 1);

    delete_skill(&pool, "test-skill").await.unwrap();
    let gone = get_skill(&pool, "test-skill").await.unwrap();
    assert!(gone.is_none());
}

#[tokio::test]
async fn skill_resolution() {
    dotenvy::dotenv().ok();
    let pool = test_pool();
    flush_test_db(&pool).await;

    let s1 = new_skill("s1", "Skill 1", "content1", "", vec!["rust".into()], Default::default());
    let s2 = new_skill("s2", "Skill 2", "content2", "", vec!["python".into()], Default::default());
    create_skill(&pool, &s1).await.unwrap();
    create_skill(&pool, &s2).await.unwrap();

    // Resolve by ID
    let resolved = resolve_skills(&pool, &["s1".into()], &[]).await.unwrap();
    assert_eq!(resolved.len(), 1);
    assert_eq!(resolved[0].id, "s1");

    // Resolve by tag
    let resolved = resolve_skills(&pool, &[], &["rust".into()]).await.unwrap();
    assert_eq!(resolved.len(), 1);
    assert_eq!(resolved[0].id, "s1");

    // Resolve both (dedup)
    let resolved = resolve_skills(&pool, &["s1".into()], &["rust".into()]).await.unwrap();
    assert_eq!(resolved.len(), 1); // s1 appears only once
}

#[tokio::test]
async fn cron_crud() {
    dotenvy::dotenv().ok();
    let pool = test_pool();
    flush_test_db(&pool).await;

    let req = CreateCronRequest {
        name: "Test Cron".into(),
        schedule: "0 * * * * *".into(),
        enabled: true,
        prompt: "say hello".into(),
        skill_ids: vec![],
        working_dir: None,
        model: None,
        max_budget_usd: None,
        output_dest: OutputDest::Redis,
        tags: vec![],
        priority: None,
    };

    let cron = create_cron(&pool, &req).await.unwrap();
    assert_eq!(cron.name, "Test Cron");
    assert!(cron.enabled);

    let all = list_crons(&pool).await.unwrap();
    assert_eq!(all.len(), 1);

    delete_cron(&pool, cron.id).await.unwrap();
    let gone = get_cron(&pool, cron.id).await.unwrap();
    assert!(gone.is_none());
}

#[tokio::test]
async fn log_append_and_get() {
    dotenvy::dotenv().ok();
    let pool = test_pool();
    flush_test_db(&pool).await;

    let job_id = uuid::Uuid::new_v4();
    append_log(&pool, job_id, "line 1").await.unwrap();
    append_log(&pool, job_id, "line 2").await.unwrap();
    append_log(&pool, job_id, "line 3").await.unwrap();

    let logs = get_logs(&pool, job_id, 0, 10).await.unwrap();
    assert_eq!(logs.len(), 3);
    assert_eq!(logs[0], "line 1");
    assert_eq!(logs[2], "line 3");

    // Offset
    let logs = get_logs(&pool, job_id, 1, 2).await.unwrap();
    assert_eq!(logs.len(), 2);
    assert_eq!(logs[0], "line 2");
}

#[tokio::test]
async fn queue_status_counts() {
    dotenvy::dotenv().ok();
    let pool = test_pool();
    flush_test_db(&pool).await;

    let req = CreateJobRequest {
        prompt: "p".into(),
        skill_ids: vec![], skill_tags: vec![], working_dir: None,
        model: None, max_budget_usd: None, allowed_tools: None,
        output_dest: OutputDest::Redis, tags: vec![], priority: None, timeout_secs: None, workspace_id: None,
    };
    submit_job(&pool, &req, JobSource::Api).await.unwrap();
    submit_job(&pool, &req, JobSource::Api).await.unwrap();

    let status = get_queue_status(&pool).await.unwrap();
    assert_eq!(status.pending, 2);
    assert_eq!(status.running, 0);

    claim_job(&pool, "w1").await.unwrap();

    let status = get_queue_status(&pool).await.unwrap();
    assert_eq!(status.pending, 1);
    assert_eq!(status.running, 1);
}
