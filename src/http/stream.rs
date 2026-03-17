use crate::app_state::AppState;
use crate::error::AppError;
use async_stream::stream;
use axum::{
    extract::{Query, State},
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse, Response,
    },
};
use std::{convert::Infallible, time::Duration};
use std::str::FromStr;

use super::dto::TopLikesQuery;
use super::top::build_top_likes_response;

const STREAM_POLL_INTERVAL_SECONDS: u64 = 5;

pub(crate) async fn stream_top_likes(
    State(state): State<AppState>,
    Query(query): Query<TopLikesQuery>,
) -> Response {
    if let Err(error) = validate_stream_query(&query) {
        return error.into_response();
    }

    let event_stream = stream! {
        let mut interval = tokio::time::interval(Duration::from_secs(STREAM_POLL_INTERVAL_SECONDS));

        loop {
            interval.tick().await;

            let response = match build_top_likes_response(&state, query.clone()).await {
                Ok(response) => response,
                Err(error) => {
                    let payload = serde_json::json!({
                        "error": {
                            "code": "STREAM_ERROR",
                            "message": error.to_string(),
                        }
                    });

                    yield Ok::<Event, Infallible>(
                        Event::default()
                            .event("error")
                            .data(payload.to_string()),
                    );
                    continue;
                }
            };

            let payload = match serde_json::to_string(&response) {
                Ok(payload) => payload,
                Err(error) => {
                    let payload = serde_json::json!({
                        "error": {
                            "code": "STREAM_ERROR",
                            "message": format!("failed to serialize stream payload: {error}"),
                        }
                    });

                    yield Ok::<Event, Infallible>(
                        Event::default()
                            .event("error")
                            .data(payload.to_string()),
                    );
                    continue;
                }
            };

            yield Ok::<Event, Infallible>(
                Event::default()
                    .event("snapshot")
                    .data(payload),
            );
        }
    };

    Sse::new(event_stream)
        .keep_alive(KeepAlive::new().interval(Duration::from_secs(15)).text("keep-alive"))
        .into_response()
}

fn validate_stream_query(query: &TopLikesQuery) -> Result<(), AppError> {
    super::helpers::parse_top_likes_window(query.window.as_deref())?;
    super::helpers::parse_top_likes_limit(query.limit)?;

    if let Some(content_type) = query.content_type.as_deref() {
        let _ = crate::domain::ContentType::from_str(content_type).map_err(AppError::from)?;
    }

    Ok(())
}
