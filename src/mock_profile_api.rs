use crate::app_state::{AppState, MockProfile};
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::Serialize;
use std::collections::HashMap;

pub async fn validate_token(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Response {
    let token = match extract_bearer_token(&headers) {
        Ok(token) => token,
        Err(response) => return response.into_response(),
    };

    match state.mock_profiles.get(token) {
        Some(profile) => success(Json(ValidateTokenResponse::from(profile))).into_response(),
        None => match generated_profile(token) {
            Some(profile) => success(Json(ValidateTokenResponse::from(&profile))).into_response(),
            None => unauthorized("invalid_token").into_response(),
        },
    }
}

#[derive(Serialize)]
pub struct ValidateTokenResponse {
    pub valid: bool,
    pub user_id: String,
    pub display_name: String,
}

impl From<&MockProfile> for ValidateTokenResponse {
    fn from(value: &MockProfile) -> Self {
        Self {
            valid: true,
            user_id: value.user_id.clone(),
            display_name: value.display_name.clone(),
        }
    }
}

#[derive(Serialize)]
struct ErrorResponse {
    valid: bool,
    error: &'static str,
}

fn extract_bearer_token(headers: &HeaderMap) -> Result<&str, (StatusCode, Json<ErrorResponse>)> {
    let authorization = headers
        .get("authorization")
        .ok_or_else(|| unauthorized("missing authorization header"))?;

    let authorization = authorization
        .to_str()
        .map_err(|_| unauthorized("authorization header must be valid UTF-8"))?;

    authorization
        .strip_prefix("Bearer ")
        .filter(|token| !token.trim().is_empty())
        .ok_or_else(|| unauthorized("authorization header must use Bearer <token> format"))
}

fn success<T>(payload: Json<T>) -> (StatusCode, Json<T>) {
    (StatusCode::OK, payload)
}

fn unauthorized(message: &'static str) -> (StatusCode, Json<ErrorResponse>) {
    (
        StatusCode::UNAUTHORIZED,
        Json(ErrorResponse {
            valid: false,
            error: message,
        }),
    )
}

fn generated_profile(token: &str) -> Option<MockProfile> {
    let suffix = token.strip_prefix("tok_user_")?;
    let numeric_id = suffix.parse::<u64>().ok()?;

    Some(MockProfile {
        user_id: generated_external_user_id(numeric_id),
        display_name: format!("Benchmark User {numeric_id}"),
    })
}

fn generated_external_user_id(numeric_id: u64) -> String {
    format!("usr_00000000-0000-0000-0000-{numeric_id:012x}")
}

pub fn build_mock_profiles() -> HashMap<String, MockProfile> {
    HashMap::from([
        (
            "tok_user_1".to_string(),
            MockProfile {
                user_id: "usr_550e8400-e29b-41d4-a716-446655440001".to_string(),
                display_name: "Test User 1".to_string(),
            },
        ),
        (
            "tok_user_2".to_string(),
            MockProfile {
                user_id: "usr_550e8400-e29b-41d4-a716-446655440002".to_string(),
                display_name: "Test User 2".to_string(),
            },
        ),
        (
            "tok_user_3".to_string(),
            MockProfile {
                user_id: "usr_550e8400-e29b-41d4-a716-446655440003".to_string(),
                display_name: "Test User 3".to_string(),
            },
        ),
        (
            "tok_user_4".to_string(),
            MockProfile {
                user_id: "usr_550e8400-e29b-41d4-a716-446655440004".to_string(),
                display_name: "Test User 4".to_string(),
            },
        ),
        (
            "tok_user_5".to_string(),
            MockProfile {
                user_id: "usr_550e8400-e29b-41d4-a716-446655440005".to_string(),
                display_name: "Test User 5".to_string(),
            },
        ),
    ])
}
