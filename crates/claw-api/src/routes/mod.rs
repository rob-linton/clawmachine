pub mod jobs;
pub mod status;
pub mod skills;
pub mod crons;
pub mod webhook;
pub mod workspaces;
pub mod pipelines;
pub mod job_templates;

use axum::Router;
use crate::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .merge(jobs::router())
        .merge(status::router())
        .merge(skills::router())
        .merge(crons::router())
        .merge(webhook::router())
        .merge(workspaces::router())
        .merge(pipelines::router())
        .merge(job_templates::router())
}
