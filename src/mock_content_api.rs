use std::collections::{HashMap, HashSet};

use crate::app_state::AppState;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Serialize;

pub async fn get_content(
    State(state): State<AppState>,
    Path((content_type, content_id)): Path<(String, String)>,
) -> Response {
    match state.mock_content_store.get(content_type.as_str()) {
        Some(content_ids) if content_ids.contains(content_id.as_str()) => (
            StatusCode::OK,
            Json(ContentResponse {
                id: content_id,
                content_type,
                title: "Mock content".to_string(),
            }),
        )
            .into_response(),
        _ => not_found("content not found").into_response(),
    }
}

pub fn build_mock_content_store() -> HashMap<String, HashSet<String>> {
    HashMap::from([
        (
            "post".to_string(),
            HashSet::from([
                "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaa1".to_string(),
                "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaa2".to_string(),
                "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaa3".to_string(),
                "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaa4".to_string(),
                "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaa5".to_string(),
            ]),
        ),
        (
            "bonus_hunter".to_string(),
            HashSet::from([
                "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbb1".to_string(),
                "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbb2".to_string(),
                "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbb3".to_string(),
                "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbb4".to_string(),
                "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbb5".to_string(),
            ]),
        ),
        (
            "top_picks".to_string(),
            HashSet::from([
                "cccccccc-cccc-cccc-cccc-ccccccccccc1".to_string(),
                "cccccccc-cccc-cccc-cccc-ccccccccccc2".to_string(),
                "cccccccc-cccc-cccc-cccc-ccccccccccc3".to_string(),
                "cccccccc-cccc-cccc-cccc-ccccccccccc4".to_string(),
                "cccccccc-cccc-cccc-cccc-ccccccccccc5".to_string(),
            ]),
        ),
    ])
}

#[derive(Serialize)]
struct ContentResponse {
    id: String,
    content_type: String,
    title: String,
}

#[derive(Serialize)]
struct ErrorResponse {
    error: &'static str,
}

fn not_found(message: &'static str) -> (StatusCode, Json<ErrorResponse>) {
    (StatusCode::NOT_FOUND, Json(ErrorResponse { error: message }))
}
