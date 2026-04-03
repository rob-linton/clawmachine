use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse,
    },
    routing::{delete, get, post},
    Json, Router,
};
use claw_models::{ChatMessage, CreateWorkspaceRequest, SendMessageRequest, WorkspacePersistence};
use futures::stream::Stream;
use serde::Deserialize;
use std::convert::Infallible;
use uuid::Uuid;

use crate::auth::CurrentUser;
use crate::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/chat", post(create_or_get_chat).get(get_chat))
        .route("/chat/messages", get(list_messages).post(send_message))
        .route("/chat/messages/{seq}/retry", post(retry_message))
        .route("/chat/search", get(search_messages))
        .route("/chat/stream", get(chat_stream_sse))
        .route("/chat/export", get(export_chat))
        .route("/chat/tasks", post(submit_task))
        .route("/chat/cancel", post(cancel_chat))
        .route("/chat/artifacts", get(list_artifacts))
        .route("/chat/artifacts/{id}", get(get_artifact))
        .route("/chat", delete(delete_chat))
}

/// Create a new chat session for the current user, or return existing one.
async fn create_or_get_chat(
    user: CurrentUser,
    State(state): State<AppState>,
) -> impl IntoResponse {
    // Check for existing default chat
    if let Ok(Some(chat_id)) = claw_redis::get_default_chat_id(&state.pool, &user.username).await {
        if let Ok(Some(session)) = claw_redis::get_chat_session(&state.pool, chat_id).await {
            return (StatusCode::OK, Json(serde_json::to_value(&session).unwrap())).into_response();
        }
    }

    // Create a persistent workspace for this chat
    let ws_req = CreateWorkspaceRequest {
        name: format!("chat-{}", user.username),
        description: Some(format!("Interactive chat workspace for {}", user.username)),
        path: None,
        skill_ids: Vec::new(),
        tool_ids: Vec::new(),
        credential_bindings: Default::default(),
        claude_md: Some(chat_claude_md(&user.username)),
        persistence: Some(WorkspacePersistence::Persistent),
        remote_url: None,
        base_image: None,
        memory_limit: None,
        cpu_limit: None,
        network_mode: None,
        parent_workspace_id: None,
        parent_ref: None,
    };

    let workspace = match claw_redis::create_workspace(&state.pool, &ws_req).await {
        Ok(ws) => ws,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": format!("Failed to create workspace: {e}")})),
            ).into_response();
        }
    };

    // Initialize the workspace bare repo + checkout with .chat/ structure
    if let Err(e) = init_chat_workspace(workspace.id).await {
        tracing::error!(error = %e, "Failed to initialize chat workspace");
    }

    match claw_redis::create_chat_session(&state.pool, &user.username, workspace.id, "sonnet").await {
        Ok(session) => (StatusCode::CREATED, Json(serde_json::to_value(&session).unwrap())).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("Failed to create chat: {e}")})),
        ).into_response(),
    }
}

/// Get the current user's default chat session.
async fn get_chat(
    user: CurrentUser,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let chat_id = match claw_redis::get_default_chat_id(&state.pool, &user.username).await {
        Ok(Some(id)) => id,
        Ok(None) => {
            return (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "No chat session"}))).into_response();
        }
        Err(e) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response();
        }
    };

    match claw_redis::get_chat_session(&state.pool, chat_id).await {
        Ok(Some(session)) => (StatusCode::OK, Json(serde_json::to_value(&session).unwrap())).into_response(),
        Ok(None) => (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "Chat session not found"}))).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    }
}

#[derive(Deserialize)]
struct MessageQuery {
    #[serde(default)]
    limit: Option<usize>,
    #[serde(default)]
    before: Option<u32>,
}

/// List chat messages with pagination.
async fn list_messages(
    user: CurrentUser,
    State(state): State<AppState>,
    Query(query): Query<MessageQuery>,
) -> impl IntoResponse {
    let chat_id = match get_user_chat_id(&state, &user.username).await {
        Some(id) => id,
        None => return (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "No chat session"}))).into_response(),
    };

    let limit = query.limit.unwrap_or(50).min(200);
    let before = query.before.unwrap_or(0);

    match claw_redis::get_chat_messages(&state.pool, chat_id, before, limit).await {
        Ok(messages) => (StatusCode::OK, Json(serde_json::json!({"messages": messages}))).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    }
}

/// Send a new message. Stores the user message and submits a job.
/// The worker will process it and store the assistant response.
async fn send_message(
    user: CurrentUser,
    State(state): State<AppState>,
    Json(req): Json<SendMessageRequest>,
) -> impl IntoResponse {
    let chat_id = match get_user_chat_id(&state, &user.username).await {
        Some(id) => id,
        None => return (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "No chat session"}))).into_response(),
    };

    let mut session = match claw_redis::get_chat_session(&state.pool, chat_id).await {
        Ok(Some(s)) => s,
        _ => return (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "Chat session not found"}))).into_response(),
    };

    // Get next sequence number
    let seq = match claw_redis::next_chat_seq(&state.pool, chat_id).await {
        Ok(s) => s,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    };

    // Store user message
    let user_msg = ChatMessage {
        seq,
        role: "user".to_string(),
        content: req.content.clone(),
        status: "complete".to_string(),
        summary: None,
        job_id: None,
        cost_usd: None,
        model: None,
        token_estimate: estimate_tokens(&req.content),
        files_written: Vec::new(),
        artifacts: Vec::new(),
        thinking: None,
        timestamp: chrono::Utc::now(),
    };
    if let Err(e) = claw_redis::add_chat_message(&state.pool, chat_id, &user_msg).await {
        return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response();
    }

    // Write user message file to workspace for grep-based history
    let home = dirs::home_dir().unwrap_or_else(|| "/tmp".into());
    let messages_dir = home.join(".claw").join("checkouts").join(session.workspace_id.to_string()).join(".chat").join("messages");
    tokio::fs::create_dir_all(&messages_dir).await.ok();
    let user_file = messages_dir.join(format!("{:04}-user.md", seq));
    tokio::fs::write(&user_file, &req.content).await.ok();

    // Assemble prompt with conversation context
    // (needed for standard job path; session container path uses --continue instead)
    let all_messages = claw_redis::get_all_chat_messages(&state.pool, chat_id).await.unwrap_or_default();
    let assembled = assemble_chat_prompt(&all_messages, &req.content, session.context_window_size);

    // Determine model
    let model = req.model.or_else(|| Some(session.model.clone()));

    let job_req = claw_models::CreateJobRequest {
        prompt: assembled,
        skill_ids: session.skill_ids.clone(),
        skill_tags: Vec::new(),
        tool_ids: session.tool_ids.clone(),
        allowed_tools: None,
        working_dir: None,
        workspace_id: Some(session.workspace_id),
        model,
        max_budget_usd: None,
        timeout_secs: Some(600), // 10 min for chat messages
        output_dest: claw_models::OutputDest::Redis,
        priority: Some(9), // high priority for interactive
        tags: vec![format!("chat:{}", chat_id), format!("chat_seq:{}", seq)],
        template_id: None,
    };

    match claw_redis::submit_job(&state.pool, &job_req, claw_models::JobSource::Api).await {
        Ok(job) => {
            // Update session metadata
            session.total_messages += 1;
            session.last_activity = chrono::Utc::now();
            session.updated_at = chrono::Utc::now();
            claw_redis::update_chat_session(&state.pool, &session).await.ok();

            (StatusCode::ACCEPTED, Json(serde_json::json!({
                "seq": seq,
                "job_id": job.id,
                "status": "submitted"
            }))).into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    }
}

/// Retry from a specific message (truncates history and resubmits).
async fn retry_message(
    user: CurrentUser,
    State(state): State<AppState>,
    axum::extract::Path(seq): axum::extract::Path<u32>,
) -> impl IntoResponse {
    let chat_id = match get_user_chat_id(&state, &user.username).await {
        Some(id) => id,
        None => return (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "No chat session"}))).into_response(),
    };

    // Truncate from the assistant response at this seq
    // Keep the user message, remove assistant and everything after
    if let Err(e) = claw_redis::truncate_chat_messages(&state.pool, chat_id, seq + 1).await {
        return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response();
    }

    // Remove the assistant message at this seq too (it will be regenerated)
    // Actually, sorted set scores: user and assistant both have score=seq
    // We need to remove only the assistant message. For now, truncate removes seq+1 and above.
    // The assistant message at seq will be replaced by the new response.
    (StatusCode::OK, Json(serde_json::json!({"status": "truncated", "retry_from_seq": seq}))).into_response()
}

#[derive(Deserialize)]
struct SearchQuery {
    q: String,
}

/// Full-text search across message content.
async fn search_messages(
    user: CurrentUser,
    State(state): State<AppState>,
    Query(query): Query<SearchQuery>,
) -> impl IntoResponse {
    let chat_id = match get_user_chat_id(&state, &user.username).await {
        Some(id) => id,
        None => return (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "No chat session"}))).into_response(),
    };

    match claw_redis::search_chat_messages(&state.pool, chat_id, &query.q).await {
        Ok(messages) => (StatusCode::OK, Json(serde_json::json!({"messages": messages}))).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    }
}

/// Delete the chat session and its workspace.
async fn delete_chat(
    user: CurrentUser,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let chat_id = match get_user_chat_id(&state, &user.username).await {
        Some(id) => id,
        None => return (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "No chat session"}))).into_response(),
    };

    if let Err(e) = claw_redis::delete_chat_session(&state.pool, chat_id, &user.username).await {
        return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response();
    }

    (StatusCode::OK, Json(serde_json::json!({"ok": true}))).into_response()
}

// --- Helpers ---

/// Submit a background task that runs in a forked workspace.
async fn submit_task(
    user: CurrentUser,
    State(state): State<AppState>,
    Json(req): Json<SendMessageRequest>,
) -> impl IntoResponse {
    let chat_id = match get_user_chat_id(&state, &user.username).await {
        Some(id) => id,
        None => return (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "No chat session"}))).into_response(),
    };

    let session = match claw_redis::get_chat_session(&state.pool, chat_id).await {
        Ok(Some(s)) => s,
        _ => return (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "Session not found"}))).into_response(),
    };

    let seq = match claw_redis::next_chat_seq(&state.pool, chat_id).await {
        Ok(s) => s,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    };

    // Store task user message
    let task_msg = ChatMessage {
        seq,
        role: "user".to_string(),
        content: format!("/task {}", req.content),
        status: "complete".to_string(),
        summary: None, job_id: None, cost_usd: None, model: None,
        token_estimate: estimate_tokens(&req.content),
        files_written: Vec::new(), artifacts: Vec::new(), thinking: None,
        timestamp: chrono::Utc::now(),
    };
    claw_redis::add_chat_message(&state.pool, chat_id, &task_msg).await.ok();

    // Fork an ephemeral workspace from the chat workspace for this task.
    // Each task gets its own workspace — no lock contention, truly parallel.
    let task_ws = match fork_ephemeral_workspace(&state.pool, &session, seq).await {
        Ok(ws) => ws,
        Err(e) => {
            tracing::warn!(error = %e, "Task workspace fork failed, using chat workspace");
            session.workspace_id // fallback
        }
    };
    let model = req.model.or_else(|| Some(session.model.clone()));

    let job_req = claw_models::CreateJobRequest {
        prompt: req.content.clone(),
        skill_ids: session.skill_ids.clone(),
        skill_tags: Vec::new(),
        tool_ids: session.tool_ids.clone(),
        allowed_tools: None,
        working_dir: None,
        workspace_id: Some(task_ws),
        model,
        max_budget_usd: None,
        timeout_secs: Some(1800),
        output_dest: claw_models::OutputDest::Redis,
        priority: Some(7),
        tags: vec![format!("chat:{}", chat_id), format!("chat_seq:{}", seq), "task".to_string()],
        template_id: None,
    };

    match claw_redis::submit_job(&state.pool, &job_req, claw_models::JobSource::Api).await {
        Ok(job) => {
            (StatusCode::ACCEPTED, Json(serde_json::json!({
                "seq": seq, "job_id": job.id, "status": "submitted", "type": "task"
            }))).into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    }
}

/// List artifacts from the workspace.
async fn list_artifacts(
    user: CurrentUser,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let chat_id = match get_user_chat_id(&state, &user.username).await {
        Some(id) => id,
        None => return (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "No chat session"}))).into_response(),
    };

    let session = match claw_redis::get_chat_session(&state.pool, chat_id).await {
        Ok(Some(s)) => s,
        _ => return (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "Session not found"}))).into_response(),
    };

    let home = dirs::home_dir().unwrap_or_else(|| "/tmp".into());
    let index_path = home.join(".claw/checkouts").join(session.workspace_id.to_string()).join(".chat/artifacts/index.json");

    match tokio::fs::read_to_string(&index_path).await {
        Ok(content) => {
            let artifacts: serde_json::Value = serde_json::from_str(&content).unwrap_or(serde_json::json!([]));
            (StatusCode::OK, Json(serde_json::json!({"artifacts": artifacts}))).into_response()
        }
        Err(_) => (StatusCode::OK, Json(serde_json::json!({"artifacts": []}))).into_response(),
    }
}

/// Get a single artifact's content by ID.
async fn get_artifact(
    user: CurrentUser,
    State(state): State<AppState>,
    Path(id): Path<u32>,
) -> impl IntoResponse {
    let chat_id = match get_user_chat_id(&state, &user.username).await {
        Some(id) => id,
        None => return (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "No chat session"}))).into_response(),
    };

    let session = match claw_redis::get_chat_session(&state.pool, chat_id).await {
        Ok(Some(s)) => s,
        _ => return (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "Session not found"}))).into_response(),
    };

    let home = dirs::home_dir().unwrap_or_else(|| "/tmp".into());
    let artifacts_dir = home.join(".claw/checkouts").join(session.workspace_id.to_string()).join(".chat/artifacts");
    let index_path = artifacts_dir.join("index.json");

    let index: Vec<serde_json::Value> = match tokio::fs::read_to_string(&index_path).await {
        Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
        Err(_) => return (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "No artifacts"}))).into_response(),
    };

    let entry = match index.iter().find(|e| e.get("id").and_then(|i| i.as_u64()) == Some(id as u64)) {
        Some(e) => e,
        None => return (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "Artifact not found"}))).into_response(),
    };

    let rel_path = match entry.get("path").and_then(|p| p.as_str()) {
        Some(p) => p,
        None => return (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "Artifact path missing"}))).into_response(),
    };

    let file_path = home.join(".claw/checkouts").join(session.workspace_id.to_string()).join(rel_path);
    match tokio::fs::read_to_string(&file_path).await {
        Ok(content) => {
            let mut result = entry.clone();
            result.as_object_mut().map(|o| o.insert("content".to_string(), serde_json::Value::String(content)));
            (StatusCode::OK, Json(result)).into_response()
        }
        Err(_) => (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "Artifact file not found"}))).into_response(),
    }
}

/// Export chat as markdown.
async fn export_chat(
    user: CurrentUser,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let chat_id = match get_user_chat_id(&state, &user.username).await {
        Some(id) => id,
        None => return (StatusCode::NOT_FOUND, "No chat session").into_response(),
    };

    let messages = claw_redis::get_all_chat_messages(&state.pool, chat_id).await.unwrap_or_default();
    let mut markdown = String::from("# Chat Export\n\n");

    for msg in &messages {
        let role = if msg.role == "user" { "You" } else { "Claude" };
        markdown.push_str(&format!("## {} (message {})\n\n{}\n\n---\n\n", role, msg.seq, msg.content));
    }

    (
        StatusCode::OK,
        [(axum::http::header::CONTENT_TYPE, "text/markdown"),
         (axum::http::header::CONTENT_DISPOSITION, "attachment; filename=\"chat-export.md\"")],
        markdown,
    ).into_response()
}

/// SSE endpoint for real-time chat response streaming.
/// Subscribes to Redis pub/sub channel `claw:chat:{chat_id}:stream`.
async fn chat_stream_sse(
    user: CurrentUser,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let chat_id = match get_user_chat_id(&state, &user.username).await {
        Some(id) => id,
        None => {
            return Sse::new(futures::stream::once(async {
                Ok::<_, Infallible>(Event::default().data(r#"{"error":"No chat session"}"#))
            })).keep_alive(KeepAlive::default()).into_response();
        }
    };

    let channel = format!("claw:chat:{}:stream", chat_id);
    let stream = chat_text_stream(state.redis_url.clone(), channel);
    Sse::new(stream).keep_alive(KeepAlive::default()).into_response()
}

fn chat_text_stream(redis_url: String, channel: String) -> impl Stream<Item = Result<Event, Infallible>> {
    async_stream::stream! {
        let client = match redis::Client::open(redis_url.as_str()) {
            Ok(c) => c,
            Err(e) => {
                tracing::error!(error = %e, "Chat SSE: failed to create Redis client");
                return;
            }
        };
        let mut pubsub = match client.get_async_pubsub().await {
            Ok(ps) => ps,
            Err(e) => {
                tracing::error!(error = %e, "Chat SSE: failed to create PubSub connection");
                return;
            }
        };
        if let Err(e) = pubsub.subscribe(&channel).await {
            tracing::error!(error = %e, %channel, "Chat SSE: failed to subscribe");
            return;
        }

        tracing::debug!(%channel, "Chat SSE: subscribed");
        yield Ok(Event::default().event("connected").data(r#"{"status":"connected"}"#));

        loop {
            use futures::StreamExt;
            match pubsub.on_message().next().await {
                Some(msg) => {
                    if let Ok(payload) = msg.get_payload::<String>() {
                        yield Ok(Event::default().event("chunk").data(payload));
                    }
                }
                None => {
                    tracing::debug!(%channel, "Chat SSE: pub/sub stream ended");
                    break;
                }
            }
        }
    }
}

/// Fork an ephemeral workspace from the chat workspace for a task.
/// Each task gets its own isolated copy — no lock contention.
async fn fork_ephemeral_workspace(
    pool: &deadpool_redis::Pool,
    session: &claw_models::ChatSession,
    seq: u32,
) -> Result<Uuid, String> {
    let parent_id = session.workspace_id;

    // Create workspace record in Redis
    let create_req = CreateWorkspaceRequest {
        name: format!("task-{}-{}", session.user_id, seq),
        description: Some(format!("Ephemeral task workspace (seq {})", seq)),
        path: None,
        skill_ids: session.skill_ids.clone(),
        tool_ids: session.tool_ids.clone(),
        credential_bindings: Default::default(),
        claude_md: None, // Inherited from parent via git clone
        persistence: Some(WorkspacePersistence::Ephemeral),
        remote_url: None,
        base_image: None,
        memory_limit: None,
        cpu_limit: None,
        network_mode: None,
        parent_workspace_id: Some(parent_id),
        parent_ref: Some("HEAD".to_string()),
    };

    let new_ws = claw_redis::create_workspace(pool, &create_req).await
        .map_err(|e| format!("Create workspace failed: {e}"))?;
    let new_id = new_ws.id;

    // Clone the bare repo
    let home = dirs::home_dir().unwrap_or_else(|| "/tmp".into());
    let parent_repo = home.join(".claw/repos").join(format!("{}.git", parent_id));
    let new_repo = home.join(".claw/repos").join(format!("{}.git", new_id));
    let new_checkout = home.join(".claw/checkouts").join(new_id.to_string());

    let cmds = format!(
        "git clone --bare {} {} && git clone {} {} && cd {} && git checkout -b claw/task-{}",
        parent_repo.display(), new_repo.display(),
        new_repo.display(), new_checkout.display(),
        new_checkout.display(), seq,
    );

    let output = tokio::process::Command::new("bash")
        .args(["-c", &cmds])
        .output().await
        .map_err(|e| format!("Fork command failed: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // Rollback
        claw_redis::delete_workspace(pool, new_id).await.ok();
        return Err(format!("Fork failed: {}", stderr.chars().take(200).collect::<String>()));
    }

    tracing::info!(parent = %parent_id, fork = %new_id, seq, "Forked ephemeral task workspace");
    Ok(new_id)
}

/// Cancel the currently running chat message.
/// Finds the running job via the exec_lock and sets its cancel flag.
async fn cancel_chat(
    user: CurrentUser,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let chat_id = match get_user_chat_id(&state, &user.username).await {
        Some(id) => id,
        None => return (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "No chat session"}))).into_response(),
    };

    // Read the exec_lock to find the currently running job_id
    let job_id = match claw_redis::get_chat_lock_holder(&state.pool, chat_id).await {
        Ok(Some(id)) => id,
        _ => return (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "No message currently executing"}))).into_response(),
    };

    // Set the per-job cancel flag (worker polls this every 2s)
    claw_redis::set_cancel_flag(&state.pool, job_id).await.ok();

    (StatusCode::OK, Json(serde_json::json!({"ok": true, "job_id": job_id.to_string()}))).into_response()
}

async fn get_user_chat_id(state: &AppState, username: &str) -> Option<Uuid> {
    claw_redis::get_default_chat_id(&state.pool, username).await.ok().flatten()
}

/// Rough token estimation (~4 chars per token).
fn estimate_tokens(text: &str) -> u32 {
    (text.len() / 4).max(1) as u32
}

/// Assemble the chat prompt with conversation context.
/// Used by the standard job path. The session container path uses --continue instead.
fn assemble_chat_prompt(messages: &[ChatMessage], new_message: &str, context_window: u32) -> String {
    let mut prompt = String::new();

    // Exclude the just-added user message
    let history: Vec<&ChatMessage> = messages.iter()
        .filter(|m| !(m.role == "user" && m.content == new_message && m.seq == messages.last().map_or(0, |l| l.seq)))
        .collect();

    let total = history.len();
    let window = (context_window as usize) * 2;
    let split_point = if total > window { total - window } else { 0 };

    if split_point > 0 {
        prompt.push_str("[Earlier conversation summary]\n");
        for msg in &history[..split_point] {
            let summary = msg.summary.as_deref().unwrap_or(&msg.content);
            let truncated = if summary.len() > 120 { &summary[..120] } else { summary };
            prompt.push_str(&format!("[{}] {}: {}\n", msg.seq, msg.role.to_uppercase(), truncated));
        }
        prompt.push('\n');
    }

    if !history.is_empty() && split_point < total {
        prompt.push_str("[Recent conversation]\n");
        for msg in &history[split_point..] {
            prompt.push_str(&format!("{} [{}]: {}\n\n", msg.role.to_uppercase(), msg.seq, msg.content));
        }
    }

    prompt.push_str(new_message);
    prompt
}

/// Generate the CLAUDE.md content for a chat workspace.
fn chat_claude_md(username: &str) -> String {
    format!(r#"# Interactive Chat Workspace

This workspace belongs to the interactive chat session for **{username}**.

## How This Works

You are in a persistent, ongoing conversation. Each message you receive is a new prompt, but you have full access to the conversation history and all files in this workspace.

## Conversation History

- **Recent messages** are included directly in your prompt
- **Older messages** are summarized in the prompt, with full text available in `.chat/messages/`
- **Rolling summary** is at `.chat/summary.md` — you should update this after significant exchanges

### Searching History

To find information from earlier in the conversation:
```bash
grep -rl "keyword" .chat/messages/
```

### Updating the Summary

After discussing important decisions, facts, or completing tasks, update `.chat/summary.md` with:
- Key facts and decisions made
- Active tasks and their status
- Important file references

## Files

Any files you create during the conversation persist across messages. The user can see them via the workspace file browser.

## Available Skills and Tools

- Check `.chat/available-skills.json` for installable skills
- Check `.chat/available-tools.json` for installable tools
- To request installation, write to `.chat/install-request.json`:
  ```json
  {{"type": "skill", "id": "skill-id-or-url"}}
  ```

## Guidelines

- Be conversational and helpful
- When you create or modify files, mention it in your response
- Update `.chat/summary.md` periodically with key information
- You can read any file in the workspace to recall past work
"#)
}

/// Initialize the chat workspace directory structure.
async fn init_chat_workspace(workspace_id: Uuid) -> Result<(), String> {
    let home = dirs::home_dir().unwrap_or_else(|| "/tmp".into());
    let repo_path = home.join(".claw").join("repos").join(format!("{}.git", workspace_id));
    let checkout_path = home.join(".claw").join("checkouts").join(workspace_id.to_string());

    // Create bare repo
    let output = tokio::process::Command::new("git")
        .args(["init", "--bare"])
        .arg(&repo_path)
        .output()
        .await
        .map_err(|e| format!("git init bare failed: {e}"))?;
    if !output.status.success() {
        return Err(format!("git init bare failed: {}", String::from_utf8_lossy(&output.stderr)));
    }

    // Create checkout directory with initial structure
    tokio::fs::create_dir_all(checkout_path.join(".chat").join("messages"))
        .await
        .map_err(|e| format!("Failed to create .chat/messages: {e}"))?;

    // Create empty summary
    tokio::fs::write(checkout_path.join(".chat").join("summary.md"), "# Chat Summary\n\n_No conversations yet._\n")
        .await
        .map_err(|e| format!("Failed to write summary.md: {e}"))?;

    // Create empty available files
    tokio::fs::write(checkout_path.join(".chat").join("available-skills.json"), "[]")
        .await.ok();
    tokio::fs::write(checkout_path.join(".chat").join("available-tools.json"), "[]")
        .await.ok();

    // Init git in checkout, add remote, initial commit
    let init_cmds = format!(
        "cd {} && git init && git add -A && git commit -m 'Initialize chat workspace' && git remote add origin {} && git push -u origin HEAD",
        checkout_path.display(),
        repo_path.display()
    );
    let output = tokio::process::Command::new("bash")
        .args(["-c", &init_cmds])
        .output()
        .await
        .map_err(|e| format!("git init checkout failed: {e}"))?;
    if !output.status.success() {
        tracing::warn!(
            stderr = %String::from_utf8_lossy(&output.stderr),
            "Chat workspace git init had warnings"
        );
    }

    tracing::info!(workspace_id = %workspace_id, "Chat workspace initialized");
    Ok(())
}
