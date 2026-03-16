pub mod job;
pub mod skill;
pub mod events;
pub mod cron_schedule;
pub mod workspace;
pub mod pipeline;
pub mod job_template;

pub use job::*;
pub use skill::*;
pub use events::*;
pub use cron_schedule::*;
pub use workspace::*;
pub use pipeline::*;
pub use job_template::*;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn job_status_serializes_lowercase() {
        let status = JobStatus::Completed;
        let json = serde_json::to_string(&status).unwrap();
        assert_eq!(json, "\"completed\"");

        let parsed: JobStatus = serde_json::from_str("\"pending\"").unwrap();
        assert_eq!(parsed, JobStatus::Pending);
    }

    #[test]
    fn output_dest_tagged_json() {
        let redis = OutputDest::Redis;
        let json = serde_json::to_string(&redis).unwrap();
        assert!(json.contains("\"type\":\"redis\""));

        let webhook = OutputDest::Webhook { url: "https://example.com".into() };
        let json = serde_json::to_string(&webhook).unwrap();
        assert!(json.contains("\"type\":\"webhook\""));
        assert!(json.contains("example.com"));

        // Round-trip
        let parsed: OutputDest = serde_json::from_str(&json).unwrap();
        match parsed {
            OutputDest::Webhook { url } => assert_eq!(url, "https://example.com"),
            _ => panic!("Expected Webhook"),
        }
    }

    #[test]
    fn create_job_request_defaults() {
        let json = r#"{"prompt": "hello"}"#;
        let req: CreateJobRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.prompt, "hello");
        assert!(req.skill_ids.is_empty());
        assert!(req.tags.is_empty());
        assert!(req.model.is_none());
        assert!(req.priority.is_none());
    }

    #[test]
    fn create_job_request_full() {
        let json = r#"{
            "prompt": "do stuff",
            "skill_ids": ["code-review"],
            "model": "sonnet",
            "priority": 8,
            "tags": ["urgent"],
            "output_dest": {"type": "file", "path": "/tmp/out"}
        }"#;
        let req: CreateJobRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.skill_ids, vec!["code-review"]);
        assert_eq!(req.priority, Some(8));
        match req.output_dest {
            OutputDest::File { path } => assert_eq!(path.to_str().unwrap(), "/tmp/out"),
            _ => panic!("Expected File"),
        }
    }

    #[test]
    fn job_source_serializes() {
        let json = serde_json::to_string(&JobSource::FileWatcher).unwrap();
        assert_eq!(json, "\"filewatcher\"");

        let parsed: JobSource = serde_json::from_str("\"cron\"").unwrap();
        assert_eq!(parsed, JobSource::Cron);
    }

    #[test]
    fn cron_schedule_defaults() {
        let json = r#"{"name":"test","schedule":"0 * * * * *","prompt":"hi"}"#;
        let req: CreateCronRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.name, "test");
        assert!(req.enabled); // default true
        assert!(req.skill_ids.is_empty());
    }

    #[test]
    fn queue_status_from_json() {
        let json = r#"{"pending":5,"running":2,"completed":10,"failed":1}"#;
        let status: QueueStatus = serde_json::from_str(json).unwrap();
        assert_eq!(status.pending, 5);
        assert_eq!(status.running, 2);
    }
}
