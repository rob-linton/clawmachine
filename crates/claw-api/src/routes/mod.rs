pub mod jobs;
pub mod status;
pub mod skills;
pub mod tools;
pub mod credentials;
pub mod crons;
pub mod webhook;
pub mod workspaces;
pub mod pipelines;
pub mod job_templates;
pub mod events;
pub mod config;
pub mod docker;
pub mod auth_routes;
pub mod oauth_login;

use axum::Router;
use crate::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .merge(jobs::router())
        .merge(status::router())
        .merge(skills::router())
        .merge(tools::router())
        .merge(credentials::router())
        .merge(crons::router())
        .merge(webhook::router())
        .merge(workspaces::router())
        .merge(pipelines::router())
        .merge(job_templates::router())
        .merge(events::router())
        .merge(config::router())
        .merge(docker::router())
        .merge(auth_routes::router())
        .merge(oauth_login::router())
}
