use crate::app_state::AppState;
use crate::domain::{ContentId, ContentType};
use crate::error::AppError;
use crate::infra::metrics::{
    record_sse_connection_close, record_sse_connection_open, record_sse_event_sent,
};
use crate::integrations::sse_events::{current_timestamp, LikeEvent};
use async_stream::stream;
use axum::{
    extract::{Query, State},
    response::{
        sse::{Event, Sse},
        IntoResponse, Response,
    },
};
use futures_util::StreamExt;
use serde_json::json;
use std::{convert::Infallible, str::FromStr, time::Duration};

use super::dto::LikeEventsStreamQuery;

const LIKE_EVENTS_STREAM_NAME: &str = "like_events";

pub(crate) async fn stream_like_events(
    State(state): State<AppState>,
    Query(query): Query<LikeEventsStreamQuery>,
) -> Response {
    let (content_type, content_id) = match validate_stream_query(&query) {
        Ok(values) => values,
        Err(error) => return error.into_response(),
    };

    let mut pubsub = match state.like_events.subscribe(&content_type, &content_id).await {
        Ok(pubsub) => pubsub,
        Err(error) => {
            return AppError::dependency_unavailable(
                "DEPENDENCY_UNAVAILABLE",
                format!("failed to subscribe to like events: {error}"),
            )
            .into_response();
        }
    };

    record_sse_connection_open(LIKE_EVENTS_STREAM_NAME);

    let event_stream = stream! {
        let _connection_guard = SseConnectionGuard::new(LIKE_EVENTS_STREAM_NAME);
        let mut shutdown = state.shutdown_signal.subscribe();
        let mut messages = pubsub.on_message();
        let mut heartbeat = tokio::time::interval(Duration::from_secs(state.sse_heartbeat_interval_seconds));
        heartbeat.tick().await;

        loop {
            tokio::select! {
                _ = shutdown.changed() => {
                    if *shutdown.borrow() {
                        let payload = json!({
                            "event": "shutdown",
                            "timestamp": current_timestamp(),
                        });

                        record_sse_event_sent(LIKE_EVENTS_STREAM_NAME, "shutdown");
                        yield Ok::<Event, Infallible>(Event::default().data(payload.to_string()));
                        break;
                    }
                }
                _ = heartbeat.tick() => {
                    let payload = json!({
                        "event": "heartbeat",
                        "timestamp": current_timestamp(),
                    });

                    record_sse_event_sent(LIKE_EVENTS_STREAM_NAME, "heartbeat");
                    yield Ok::<Event, Infallible>(Event::default().data(payload.to_string()));
                }
                maybe_message = messages.next() => {
                    match maybe_message {
                        Some(message) => {
                            let payload: Result<String, _> = message.get_payload();

                            let event = match payload
                                .ok()
                                .and_then(|payload| serde_json::from_str::<LikeEvent>(&payload).ok()) {
                                Some(event) => event,
                                None => {
                                    let payload = json!({
                                        "event": "error",
                                        "message": "failed to decode like event",
                                    });

                                    record_sse_event_sent(LIKE_EVENTS_STREAM_NAME, "error");
                                    yield Ok::<Event, Infallible>(Event::default().data(payload.to_string()));
                                    continue;
                                }
                            };

                            let payload = json!({
                                "event": event.event,
                                "user_id": event.user_id,
                                "count": event.count,
                                "timestamp": event.timestamp,
                            });

                            let event_name = payload["event"].as_str().unwrap_or("unknown");
                            record_sse_event_sent(LIKE_EVENTS_STREAM_NAME, event_name);
                            yield Ok::<Event, Infallible>(Event::default().data(payload.to_string()));
                        }
                        None => {
                            break;
                        }
                    }
                }
            }
        }
    };

    Sse::new(event_stream).into_response()
}

struct SseConnectionGuard {
    stream: &'static str,
}

impl SseConnectionGuard {
    fn new(stream: &'static str) -> Self {
        Self { stream }
    }
}

impl Drop for SseConnectionGuard {
    fn drop(&mut self) {
        record_sse_connection_close(self.stream);
    }
}

fn validate_stream_query(
    query: &LikeEventsStreamQuery,
) -> Result<(ContentType, ContentId), AppError> {
    let content_type = ContentType::from_str(&query.content_type).map_err(AppError::from)?;
    let content_id = ContentId::from_str(&query.content_id).map_err(AppError::from)?;
    Ok((content_type, content_id))
}
