use axum::{
    extract::State,
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse,
    },
    routing::get,
    Router,
};
use futures::stream::Stream;
use std::convert::Infallible;

use crate::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/events/jobs", get(job_events_sse))
}

/// SSE endpoint that subscribes to Redis Pub/Sub channel `claw:events:jobs`
/// and forwards job update events to connected clients.
async fn job_events_sse(
    State(state): State<AppState>,
) -> impl IntoResponse {
    let stream = job_event_stream(state.redis_url);
    Sse::new(stream).keep_alive(KeepAlive::default())
}

fn job_event_stream(
    redis_url: String,
) -> impl Stream<Item = Result<Event, Infallible>> {
    async_stream::stream! {
        let client = match redis::Client::open(redis_url.as_str()) {
            Ok(c) => c,
            Err(e) => {
                tracing::error!(error = %e, "Failed to create Redis client for SSE");
                yield Ok(Event::default().data(format!(r#"{{"error":"Redis connection failed"}}"#)));
                return;
            }
        };

        let mut pubsub = match client.get_async_pubsub().await {
            Ok(ps) => ps,
            Err(e) => {
                tracing::error!(error = %e, "Failed to create PubSub for SSE");
                yield Ok(Event::default().data(format!(r#"{{"error":"PubSub failed"}}"#)));
                return;
            }
        };

        if let Err(e) = pubsub.subscribe("claw:events:jobs").await {
            tracing::error!(error = %e, "Failed to subscribe to job events");
            return;
        }

        tracing::debug!("SSE client connected to job events");
        yield Ok(Event::default().event("connected").data(r#"{"status":"connected"}"#));

        loop {
            use futures::StreamExt;
            match pubsub.on_message().next().await {
                Some(msg) => {
                    if let Ok(payload) = msg.get_payload::<String>() {
                        yield Ok(Event::default().event("job_update").data(payload));
                    }
                }
                None => {
                    tracing::debug!("SSE PubSub stream ended");
                    break;
                }
            }
        }
    }
}
