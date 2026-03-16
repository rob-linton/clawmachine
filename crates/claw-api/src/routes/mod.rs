pub mod jobs;
pub mod status;
pub mod skills;
pub mod crons;
pub mod webhook;
pub mod workspaces;

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
}
