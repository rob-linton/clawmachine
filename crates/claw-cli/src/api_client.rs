use claw_models::{CreateJobRequest, CreateJobResponse, Job, JobResultResponse};
use uuid::Uuid;

pub struct ApiClient {
    base_url: String,
    client: reqwest::Client,
}

impl ApiClient {
    pub fn new(base_url: &str) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            client: reqwest::Client::new(),
        }
    }

    pub async fn submit_job(&self, req: &CreateJobRequest) -> Result<CreateJobResponse, String> {
        let resp = self
            .client
            .post(format!("{}/api/v1/jobs", self.base_url))
            .json(req)
            .send()
            .await
            .map_err(|e| format!("Request failed: {e}"))?;

        if !resp.status().is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(format!("API error: {text}"));
        }

        resp.json().await.map_err(|e| format!("Parse error: {e}"))
    }

    pub async fn get_job(&self, id: Uuid) -> Result<Job, String> {
        let resp = self
            .client
            .get(format!("{}/api/v1/jobs/{id}", self.base_url))
            .send()
            .await
            .map_err(|e| format!("Request failed: {e}"))?;

        if resp.status().as_u16() == 404 {
            return Err(format!("Job {id} not found"));
        }
        if !resp.status().is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(format!("API error: {text}"));
        }

        resp.json().await.map_err(|e| format!("Parse error: {e}"))
    }

    pub async fn get_result(&self, id: Uuid) -> Result<JobResultResponse, String> {
        let resp = self
            .client
            .get(format!("{}/api/v1/jobs/{id}/result", self.base_url))
            .send()
            .await
            .map_err(|e| format!("Request failed: {e}"))?;

        if resp.status().as_u16() == 404 {
            return Err(format!("Result for job {id} not found (job may still be running)"));
        }
        if !resp.status().is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(format!("API error: {text}"));
        }

        resp.json().await.map_err(|e| format!("Parse error: {e}"))
    }

    pub async fn get_status(&self) -> Result<serde_json::Value, String> {
        let resp = self
            .client
            .get(format!("{}/api/v1/status", self.base_url))
            .send()
            .await
            .map_err(|e| format!("Request failed: {e}"))?;

        resp.json().await.map_err(|e| format!("Parse error: {e}"))
    }

    pub async fn list_jobs(
        &self,
        status: Option<&str>,
        limit: usize,
    ) -> Result<serde_json::Value, String> {
        let mut url = format!("{}/api/v1/jobs?limit={limit}", self.base_url);
        if let Some(s) = status {
            url.push_str(&format!("&status={s}"));
        }

        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| format!("Request failed: {e}"))?;

        resp.json().await.map_err(|e| format!("Parse error: {e}"))
    }

    pub async fn get_logs(&self, id: Uuid) -> Result<serde_json::Value, String> {
        let resp = self
            .client
            .get(format!("{}/api/v1/jobs/{id}/logs", self.base_url))
            .send()
            .await
            .map_err(|e| format!("Request failed: {e}"))?;

        resp.json().await.map_err(|e| format!("Parse error: {e}"))
    }

    pub async fn cancel_job(&self, id: Uuid) -> Result<(), String> {
        let resp = self
            .client
            .post(format!("{}/api/v1/jobs/{id}/cancel", self.base_url))
            .send()
            .await
            .map_err(|e| format!("Request failed: {e}"))?;

        if resp.status().as_u16() == 409 {
            let text = resp.text().await.unwrap_or_default();
            return Err(format!("Cannot cancel: {text}"));
        }
        if !resp.status().is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(format!("API error: {text}"));
        }
        Ok(())
    }

    pub async fn create_skill(
        &self, id: &str, name: &str, skill_type: &str, content: &str, description: &str, tags: &[String],
    ) -> Result<(), String> {
        let body = serde_json::json!({
            "id": id,
            "name": name,
            "skill_type": skill_type,
            "content": content,
            "description": description,
            "tags": tags,
        });
        let resp = self.client.post(format!("{}/api/v1/skills", self.base_url))
            .json(&body).send().await.map_err(|e| format!("Request failed: {e}"))?;
        if !resp.status().is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(format!("API error: {text}"));
        }
        Ok(())
    }

    pub async fn list_skills(&self) -> Result<serde_json::Value, String> {
        let resp = self.client.get(format!("{}/api/v1/skills", self.base_url))
            .send().await.map_err(|e| format!("Request failed: {e}"))?;
        resp.json().await.map_err(|e| format!("Parse error: {e}"))
    }

    pub async fn get_skill(&self, id: &str) -> Result<serde_json::Value, String> {
        let resp = self.client.get(format!("{}/api/v1/skills/{id}", self.base_url))
            .send().await.map_err(|e| format!("Request failed: {e}"))?;
        if resp.status().as_u16() == 404 {
            return Err(format!("Skill '{id}' not found"));
        }
        resp.json().await.map_err(|e| format!("Parse error: {e}"))
    }

    pub async fn delete_skill(&self, id: &str) -> Result<(), String> {
        let resp = self.client.delete(format!("{}/api/v1/skills/{id}", self.base_url))
            .send().await.map_err(|e| format!("Request failed: {e}"))?;
        if !resp.status().is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(format!("API error: {text}"));
        }
        Ok(())
    }

    /// Poll until the job completes, then return the result.
    pub async fn wait_for_result(&self, id: Uuid) -> Result<JobResultResponse, String> {
        loop {
            match self.get_job(id).await {
                Ok(job) => match job.status {
                    claw_models::JobStatus::Completed => {
                        return self.get_result(id).await;
                    }
                    claw_models::JobStatus::Failed => {
                        return Err(format!(
                            "Job failed: {}",
                            job.error.unwrap_or_else(|| "unknown error".into())
                        ));
                    }
                    claw_models::JobStatus::Cancelled => {
                        return Err("Job was cancelled".into());
                    }
                    _ => {
                        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                    }
                },
                Err(e) => return Err(e),
            }
        }
    }
}
