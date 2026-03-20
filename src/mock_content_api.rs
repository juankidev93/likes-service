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
        None
            if state.content_type_registry.contains(content_type.as_str())
                && uuid::Uuid::parse_str(&content_id).is_ok() =>
        {
            (
                StatusCode::OK,
                Json(ContentResponse {
                    id: content_id,
                    content_type,
                    title: "Mock content".to_string(),
                }),
            )
                .into_response()
        }
        _ => not_found("content not found").into_response(),
    }
}

pub fn build_mock_content_store() -> HashMap<String, HashSet<String>> {
    HashMap::from([
        (
            "post".to_string(),
            HashSet::from([
                "731b0395-4888-4822-b516-05b4b7bf2089".to_string(),
                "9601c044-6130-4ee5-a155-96570e05a02f".to_string(),
                "933dde0f-4744-4a66-9a38-bf5cb1f67553".to_string(),
                "ea0f2020-0509-45fd-adb9-24b8843055ee".to_string(),
                "bd27f926-0a00-41fd-b085-a7491e6d0902".to_string(),
                "2a656157-5284-48b5-9d76-ede492933347".to_string(),
                "4f884e5e-2f1d-4965-b0f1-16922acd91a2".to_string(),
                "ad1d9238-622c-4875-9881-5f8e19997783".to_string(),
                "c34ee1e3-7224-4a97-ba44-0993eb7a6ed8".to_string(),
                "c2b7f212-6162-4ae6-837b-16ee34cc9a50".to_string(),
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
                "c3d4e5f6-a7b8-9012-cdef-123456789012".to_string(),
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
