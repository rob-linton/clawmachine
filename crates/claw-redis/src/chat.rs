use chrono::Utc;
use claw_models::{ChatMessage, ChatSession};
use deadpool_redis::Pool;
use redis::AsyncCommands;
use uuid::Uuid;

use crate::RedisError;

const CHAT_PREFIX: &str = "claw:chat:";

// --- ChatSession ---

pub async fn create_chat_session(pool: &Pool, user_id: &str, workspace_id: Uuid, model: &str) -> Result<ChatSession, RedisError> {
    let mut conn = pool.get().await?;
    let now = Utc::now();
    let session = ChatSession {
        id: Uuid::new_v4(),
        user_id: user_id.to_string(),
        workspace_id,
        title: None,
        model: model.to_string(),
        context_window_size: 20,
        total_messages: 0,
        total_cost_usd: 0.0,
        skill_ids: Vec::new(),
        tool_ids: Vec::new(),
        created_at: now,
        updated_at: now,
        last_activity: now,
    };

    let json = serde_json::to_string(&session)?;
    let key = format!("{}{}", CHAT_PREFIX, session.id);

    redis::pipe()
        .set(&key, &json)
        .sadd(format!("claw:user:{}:chats", user_id), session.id.to_string())
        .set(format!("claw:user:{}:default_chat", user_id), session.id.to_string())
        .exec_async(&mut *conn)
        .await?;

    tracing::info!(chat_id = %session.id, user = %user_id, "Chat session created");
    Ok(session)
}

pub async fn get_chat_session(pool: &Pool, chat_id: Uuid) -> Result<Option<ChatSession>, RedisError> {
    let mut conn = pool.get().await?;
    let key = format!("{}{}", CHAT_PREFIX, chat_id);
    let json: Option<String> = conn.get(&key).await?;
    match json {
        Some(j) => Ok(Some(serde_json::from_str(&j)?)),
        None => Ok(None),
    }
}

pub async fn update_chat_session(pool: &Pool, session: &ChatSession) -> Result<(), RedisError> {
    let mut conn = pool.get().await?;
    let key = format!("{}{}", CHAT_PREFIX, session.id);
    let json = serde_json::to_string(session)?;
    let _: () = conn.set(&key, &json).await?;
    Ok(())
}

pub async fn delete_chat_session(pool: &Pool, chat_id: Uuid, user_id: &str) -> Result<(), RedisError> {
    let mut conn = pool.get().await?;
    let key = format!("{}{}", CHAT_PREFIX, chat_id);
    let messages_key = format!("{}{}:messages", CHAT_PREFIX, chat_id);

    redis::pipe()
        .del(&key)
        .del(&messages_key)
        .srem(format!("claw:user:{}:chats", user_id), chat_id.to_string())
        .del(format!("claw:user:{}:default_chat", user_id))
        .exec_async(&mut *conn)
        .await?;

    tracing::info!(chat_id = %chat_id, user = %user_id, "Chat session deleted");
    Ok(())
}

/// Get the user's default chat session ID.
pub async fn get_default_chat_id(pool: &Pool, user_id: &str) -> Result<Option<Uuid>, RedisError> {
    let mut conn = pool.get().await?;
    let key = format!("claw:user:{}:default_chat", user_id);
    let val: Option<String> = conn.get(&key).await?;
    Ok(val.and_then(|s| s.parse().ok()))
}

// --- ChatMessage ---

/// Append a message to the chat. Returns the sequence number.
pub async fn add_chat_message(pool: &Pool, chat_id: Uuid, msg: &ChatMessage) -> Result<u32, RedisError> {
    let mut conn = pool.get().await?;
    let messages_key = format!("{}{}:messages", CHAT_PREFIX, chat_id);
    let json = serde_json::to_string(msg)?;
    let _: () = conn.zadd(&messages_key, &json, msg.seq as f64).await?;
    Ok(msg.seq)
}

/// Get messages with pagination. Returns messages with seq < `before` (or all if before=0), limited to `limit`.
pub async fn get_chat_messages(pool: &Pool, chat_id: Uuid, before: u32, limit: usize) -> Result<Vec<ChatMessage>, RedisError> {
    let mut conn = pool.get().await?;
    let messages_key = format!("{}{}:messages", CHAT_PREFIX, chat_id);

    let raw: Vec<String> = if before == 0 {
        // Get latest messages (highest seq numbers)
        conn.zrevrange(&messages_key, 0, (limit as isize) - 1).await?
    } else {
        // Get messages before a specific seq
        conn.zrevrangebyscore_limit(
            &messages_key,
            format!("({}", before), // exclusive upper bound
            "-inf",
            0,
            limit as isize,
        ).await?
    };

    let mut messages: Vec<ChatMessage> = raw
        .iter()
        .filter_map(|j| serde_json::from_str(j).ok())
        .collect();
    messages.sort_by_key(|m| m.seq);
    Ok(messages)
}

/// Get all messages (for context assembly). Returns in seq order.
pub async fn get_all_chat_messages(pool: &Pool, chat_id: Uuid) -> Result<Vec<ChatMessage>, RedisError> {
    let mut conn = pool.get().await?;
    let messages_key = format!("{}{}:messages", CHAT_PREFIX, chat_id);
    let raw: Vec<String> = conn.zrangebyscore(&messages_key, "-inf", "+inf").await?;
    let messages: Vec<ChatMessage> = raw
        .iter()
        .filter_map(|j| serde_json::from_str(j).ok())
        .collect();
    Ok(messages)
}

/// Get the next sequence number for a chat.
/// Uses atomic INCR on a dedicated counter key — safe for rapid concurrent sends.
pub async fn next_chat_seq(pool: &Pool, chat_id: Uuid) -> Result<u32, RedisError> {
    let mut conn = pool.get().await?;
    let counter_key = format!("{}{}:seq_counter", CHAT_PREFIX, chat_id);
    let seq: u32 = conn.incr(&counter_key, 1).await?;
    Ok(seq)
}

/// Truncate messages at and after a given seq (for retry/edit).
pub async fn truncate_chat_messages(pool: &Pool, chat_id: Uuid, from_seq: u32) -> Result<(), RedisError> {
    let mut conn = pool.get().await?;
    let messages_key = format!("{}{}:messages", CHAT_PREFIX, chat_id);
    let _: () = conn.zrembyscore(&messages_key, from_seq as f64, "+inf").await?;
    Ok(())
}

/// Simple full-text search across message content.
pub async fn search_chat_messages(pool: &Pool, chat_id: Uuid, query: &str) -> Result<Vec<ChatMessage>, RedisError> {
    let all = get_all_chat_messages(pool, chat_id).await?;
    let query_lower = query.to_lowercase();
    Ok(all
        .into_iter()
        .filter(|m| m.content.to_lowercase().contains(&query_lower))
        .collect())
}

/// Update the summary field on an existing message.
pub async fn update_message_summary(pool: &Pool, chat_id: Uuid, seq: u32, role: &str, summary: &str) -> Result<(), RedisError> {
    let mut conn = pool.get().await?;
    let messages_key = format!("{}{}:messages", CHAT_PREFIX, chat_id);

    // Read all messages at this seq score, find the right role, update it
    let raw: Vec<String> = conn.zrangebyscore(&messages_key, seq as f64, seq as f64).await?;
    for json_str in &raw {
        if let Ok(mut msg) = serde_json::from_str::<ChatMessage>(json_str) {
            if msg.role == role {
                // Remove old, add updated
                let _: () = conn.zrem(&messages_key, json_str).await?;
                msg.summary = Some(summary.to_string());
                let new_json = serde_json::to_string(&msg)?;
                let _: () = conn.zadd(&messages_key, &new_json, seq as f64).await?;
                return Ok(());
            }
        }
    }
    Ok(())
}

// --- Streaming ---

/// Publish a chat stream chunk to Redis pub/sub for real-time UI display.
pub async fn publish_chat_stream(pool: &Pool, channel: &str, data: &str) -> Result<(), RedisError> {
    let mut conn = pool.get().await?;
    let _: () = redis::cmd("PUBLISH")
        .arg(channel)
        .arg(data)
        .query_async(&mut *conn)
        .await?;
    Ok(())
}

// --- Container Tracking ---

/// Store the active container info for a chat session.
pub async fn set_chat_container(pool: &Pool, chat_id: Uuid, container_name: &str) -> Result<(), RedisError> {
    let mut conn = pool.get().await?;
    let key = format!("{}{}:container", CHAT_PREFIX, chat_id);
    let json = serde_json::json!({
        "container_name": container_name,
        "started_at": chrono::Utc::now().to_rfc3339(),
    });
    let _: () = conn.set(&key, serde_json::to_string(&json)?).await?;
    Ok(())
}

/// Get the active container name for a chat session.
pub async fn get_chat_container(pool: &Pool, chat_id: Uuid) -> Result<Option<String>, RedisError> {
    let mut conn = pool.get().await?;
    let key = format!("{}{}:container", CHAT_PREFIX, chat_id);
    let json: Option<String> = conn.get(&key).await?;
    match json {
        Some(j) => {
            let v: serde_json::Value = serde_json::from_str(&j)?;
            Ok(v.get("container_name").and_then(|v| v.as_str()).map(|s| s.to_string()))
        }
        None => Ok(None),
    }
}

/// Delete the container tracking key.
pub async fn delete_chat_container(pool: &Pool, chat_id: Uuid) -> Result<(), RedisError> {
    let mut conn = pool.get().await?;
    let key = format!("{}{}:container", CHAT_PREFIX, chat_id);
    let _: () = conn.del(&key).await?;
    Ok(())
}
