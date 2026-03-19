use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

use crate::OutputDest;

/// Deserialize a Vec<String> that might be stored as {} (empty object) in Redis.
/// This happens when some code paths serialize empty collections as objects.
fn vec_string_or_empty<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: Deserializer<'de>,
{
    use serde::de;
    struct VecOrEmpty;
    impl<'de> de::Visitor<'de> for VecOrEmpty {
        type Value = Vec<String>;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("a sequence or empty object")
        }
        fn visit_seq<A: de::SeqAccess<'de>>(self, mut seq: A) -> Result<Vec<String>, A::Error> {
            let mut v = Vec::new();
            while let Some(s) = seq.next_element()? {
                v.push(s);
            }
            Ok(v)
        }
        fn visit_map<A: de::MapAccess<'de>>(self, _map: A) -> Result<Vec<String>, A::Error> {
            Ok(Vec::new()) // {} → empty vec
        }
    }
    deserializer.deserialize_any(VecOrEmpty)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronSchedule {
    pub id: Uuid,
    pub name: String,
    pub schedule: String,
    pub enabled: bool,
    pub prompt: String,
    #[serde(default, deserialize_with = "vec_string_or_empty")]
    pub skill_ids: Vec<String>,
    #[serde(default = "default_cron_working_dir")]
    pub working_dir: PathBuf,
    pub model: Option<String>,
    pub max_budget_usd: Option<f64>,
    #[serde(default)]
    pub output_dest: OutputDest,
    #[serde(default, deserialize_with = "vec_string_or_empty")]
    pub tags: Vec<String>,
    #[serde(default = "default_cron_priority")]
    pub priority: u8,
    pub workspace_id: Option<Uuid>,
    pub template_id: Option<Uuid>,
    pub last_run: Option<DateTime<Utc>>,
    pub last_job_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
}

fn default_cron_working_dir() -> PathBuf {
    PathBuf::from(".")
}

fn default_cron_priority() -> u8 {
    5
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateCronRequest {
    pub name: String,
    pub schedule: String,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    pub prompt: String,
    #[serde(default, deserialize_with = "vec_string_or_empty")]
    pub skill_ids: Vec<String>,
    pub working_dir: Option<PathBuf>,
    pub model: Option<String>,
    pub max_budget_usd: Option<f64>,
    #[serde(default)]
    pub output_dest: OutputDest,
    #[serde(default, deserialize_with = "vec_string_or_empty")]
    pub tags: Vec<String>,
    pub priority: Option<u8>,
    pub workspace_id: Option<Uuid>,
    pub template_id: Option<Uuid>,
}

fn default_enabled() -> bool {
    true
}
