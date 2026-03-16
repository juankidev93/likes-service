use crate::app_state::{AppState, MockProfile};
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::Serialize;

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
        None => unauthorized("invalid bearer token").into_response(),
    }
}

#[derive(Serialize)]
pub struct ValidateTokenResponse {
    pub user_id: String,
    pub display_name: String,
}

impl From<&MockProfile> for ValidateTokenResponse {
    fn from(value: &MockProfile) -> Self {
        Self {
            user_id: value.user_id.clone(),
            display_name: value.display_name.clone(),
        }
    }
}

#[derive(Serialize)]
struct ErrorResponse {
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
        Json(ErrorResponse { error: message }),
    )
}
