pub mod jobs;
pub mod status;
pub mod skills;

use axum::Router;
use crate::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .merge(jobs::router())
        .merge(status::router())
        .merge(skills::router())
}
