use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use chrono::Utc;
use claw_models::*;
use deadpool_redis::Pool;
use rand::RngCore;
use redis::AsyncCommands;
use std::collections::HashMap;

use crate::RedisError;

/// Get the encryption key from CLAW_SECRET_KEY env var.
/// Returns None if not set (credentials feature disabled).
fn get_encryption_key() -> Option<[u8; 32]> {
    let key_str = std::env::var("CLAW_SECRET_KEY").ok()?;
    let key_str = key_str.trim();
    if key_str.is_empty() {
        return None;
    }

    // Try hex decode first (64 hex chars = 32 bytes)
    if key_str.len() == 64 {
        if let Ok(bytes) = hex::decode(key_str) {
            if bytes.len() == 32 {
                let mut key = [0u8; 32];
                key.copy_from_slice(&bytes);
                return Some(key);
            }
        }
    }

    // Try base64 decode
    if let Ok(bytes) = BASE64.decode(key_str) {
        if bytes.len() == 32 {
            let mut key = [0u8; 32];
            key.copy_from_slice(&bytes);
            return Some(key);
        }
    }

    // Fall back to SHA-256 hash of the key string
    use sha2::{Digest, Sha256};
    let hash = Sha256::digest(key_str.as_bytes());
    let mut key = [0u8; 32];
    key.copy_from_slice(&hash);
    Some(key)
}

/// Encrypt a value using AES-256-GCM. Returns base64(nonce || ciphertext).
fn encrypt_value(key: &[u8; 32], plaintext: &str) -> Result<String, String> {
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|e| format!("Cipher init: {e}"))?;
    let mut nonce_bytes = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, plaintext.as_bytes())
        .map_err(|e| format!("Encrypt failed: {e}"))?;

    // Concatenate nonce + ciphertext
    let mut combined = Vec::with_capacity(12 + ciphertext.len());
    combined.extend_from_slice(&nonce_bytes);
    combined.extend_from_slice(&ciphertext);

    Ok(BASE64.encode(&combined))
}

/// Decrypt a value. Input is base64(nonce || ciphertext).
fn decrypt_value(key: &[u8; 32], encoded: &str) -> Result<String, String> {
    let combined = BASE64
        .decode(encoded)
        .map_err(|e| format!("Base64 decode: {e}"))?;
    if combined.len() < 13 {
        return Err("Ciphertext too short".into());
    }

    let (nonce_bytes, ciphertext) = combined.split_at(12);
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|e| format!("Cipher init: {e}"))?;
    let nonce = Nonce::from_slice(nonce_bytes);

    let plaintext = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|e| format!("Decrypt failed: {e}"))?;

    String::from_utf8(plaintext).map_err(|e| format!("UTF-8 decode: {e}"))
}

/// Create a credential with encrypted values.
pub async fn create_credential(
    pool: &Pool,
    req: &CreateCredentialRequest,
) -> Result<Credential, RedisError> {
    let key = get_encryption_key()
        .ok_or_else(|| RedisError::Other("CLAW_SECRET_KEY not set — cannot store credentials".into()))?;

    let mut conn = pool.get().await?;
    let now = Utc::now();

    let credential = Credential {
        id: req.id.clone(),
        name: req.name.clone(),
        description: req.description.clone(),
        keys: req.values.keys().cloned().collect(),
        created_at: now,
        updated_at: now,
    };

    // Encrypt each value
    let mut encrypted: HashMap<String, String> = HashMap::new();
    for (k, v) in &req.values {
        encrypted.insert(k.clone(), encrypt_value(&key, v).map_err(|e| RedisError::Other(e))?);
    }

    let meta_json = serde_json::to_string(&credential)?;
    let values_json = serde_json::to_string(&encrypted)?;

    redis::pipe()
        .set(format!("claw:credential:{}", req.id), &meta_json)
        .set(format!("claw:credential:{}:values", req.id), &values_json)
        .sadd("claw:credentials:index", &req.id)
        .exec_async(&mut *conn)
        .await?;

    tracing::info!(credential_id = %req.id, "Credential created");
    Ok(credential)
}

/// Get credential metadata (no values).
pub async fn get_credential(pool: &Pool, id: &str) -> Result<Option<Credential>, RedisError> {
    let mut conn = pool.get().await?;
    let json: Option<String> = conn.get(format!("claw:credential:{}", id)).await?;
    match json {
        Some(j) => Ok(Some(serde_json::from_str(&j)?)),
        None => Ok(None),
    }
}

/// Get decrypted credential values (worker-only — never expose via API).
pub async fn get_credential_values(
    pool: &Pool,
    id: &str,
) -> Result<Option<HashMap<String, String>>, RedisError> {
    let key = get_encryption_key()
        .ok_or_else(|| RedisError::Other("CLAW_SECRET_KEY not set".into()))?;

    let mut conn = pool.get().await?;
    let json: Option<String> = conn
        .get(format!("claw:credential:{}:values", id))
        .await?;

    let Some(j) = json else { return Ok(None) };

    let encrypted: HashMap<String, String> = serde_json::from_str(&j)?;
    let mut decrypted = HashMap::new();
    for (k, v) in &encrypted {
        decrypted.insert(
            k.clone(),
            decrypt_value(&key, v).map_err(|e| RedisError::Other(e))?,
        );
    }

    Ok(Some(decrypted))
}

/// List all credentials (metadata only, values masked).
pub async fn list_credentials(pool: &Pool) -> Result<Vec<CredentialResponse>, RedisError> {
    let mut conn = pool.get().await?;
    let ids: Vec<String> = redis::cmd("SMEMBERS")
        .arg("claw:credentials:index")
        .query_async(&mut *conn)
        .await
        .unwrap_or_default();

    let mut results = Vec::new();
    for id in &ids {
        let meta_key = format!("claw:credential:{}", id);
        if let Ok(json) = conn.get::<_, String>(&meta_key).await {
            if let Ok(cred) = serde_json::from_str::<Credential>(&json) {
                let masked: HashMap<String, String> = cred
                    .keys
                    .iter()
                    .map(|k| (k.clone(), "***set***".to_string()))
                    .collect();
                results.push(CredentialResponse {
                    id: cred.id,
                    name: cred.name,
                    description: cred.description,
                    keys: cred.keys,
                    masked_values: masked,
                    created_at: cred.created_at,
                    updated_at: cred.updated_at,
                });
            }
        }
    }
    results.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(results)
}

/// Update credential values (re-encrypts).
pub async fn update_credential(
    pool: &Pool,
    id: &str,
    req: &CreateCredentialRequest,
) -> Result<Credential, RedisError> {
    let key = get_encryption_key()
        .ok_or_else(|| RedisError::Other("CLAW_SECRET_KEY not set".into()))?;

    let mut conn = pool.get().await?;

    // Preserve created_at
    let existing = get_credential(pool, id).await?;
    let created_at = existing.map(|c| c.created_at).unwrap_or_else(Utc::now);

    let credential = Credential {
        id: id.to_string(),
        name: req.name.clone(),
        description: req.description.clone(),
        keys: req.values.keys().cloned().collect(),
        created_at,
        updated_at: Utc::now(),
    };

    let mut encrypted: HashMap<String, String> = HashMap::new();
    for (k, v) in &req.values {
        encrypted.insert(k.clone(), encrypt_value(&key, v).map_err(|e| RedisError::Other(e))?);
    }

    let meta_json = serde_json::to_string(&credential)?;
    let values_json = serde_json::to_string(&encrypted)?;

    redis::pipe()
        .set(format!("claw:credential:{}", id), &meta_json)
        .set(format!("claw:credential:{}:values", id), &values_json)
        .sadd("claw:credentials:index", id)
        .exec_async(&mut *conn)
        .await?;

    tracing::info!(credential_id = %id, "Credential updated");
    Ok(credential)
}

/// Delete a credential.
pub async fn delete_credential(pool: &Pool, id: &str) -> Result<(), RedisError> {
    let mut conn = pool.get().await?;
    redis::pipe()
        .del(format!("claw:credential:{}", id))
        .del(format!("claw:credential:{}:values", id))
        .srem("claw:credentials:index", id)
        .exec_async(&mut *conn)
        .await?;
    tracing::info!(credential_id = %id, "Credential deleted");
    Ok(())
}

// hex decode helper (avoid adding another dependency)
mod hex {
    pub fn decode(s: &str) -> Result<Vec<u8>, String> {
        if s.len() % 2 != 0 {
            return Err("Odd length".into());
        }
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(|e| e.to_string()))
            .collect()
    }
}
