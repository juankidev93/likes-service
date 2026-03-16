use crate::app_state::AppState;
use crate::profile_api_client::AuthError;
use axum::{
    extract::Request,
    http::{header, HeaderMap, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use serde::Serialize;

pub async fn require_auth(
    axum::extract::State(state): axum::extract::State<AppState>,
    mut request: Request,
    next: Next,
) -> Response {
    let bearer_token = match extract_bearer_token(request.headers()) {
        Ok(token) => token,
        Err(response) => return response.into_response(),
    };

    match state.profile_api_client.validate_token(bearer_token).await {
        Ok(authenticated_user) => {
            request.extensions_mut().insert(authenticated_user);
            next.run(request).await
        }
        Err(AuthError::InvalidToken) => unauthorized("invalid bearer token").into_response(),
        Err(AuthError::DependencyUnavailable(message)) => {
            dependency_unavailable(message).into_response()
        }
        Err(AuthError::NetworkError(message)) => dependency_unavailable(message).into_response(),
    }
}

#[derive(Serialize)]
struct ErrorResponse {
    error: String,
}

fn extract_bearer_token(headers: &HeaderMap) -> Result<&str, (StatusCode, Json<ErrorResponse>)> {
    let authorization = headers
        .get(header::AUTHORIZATION)
        .ok_or_else(|| unauthorized("missing authorization header"))?;

    let authorization = authorization
        .to_str()
        .map_err(|_| unauthorized("authorization header must be valid UTF-8"))?;

    authorization
        .strip_prefix("Bearer ")
        .filter(|token| !token.trim().is_empty())
        .ok_or_else(|| unauthorized("authorization header must use Bearer <token> format"))
}

fn unauthorized(message: impl Into<String>) -> (StatusCode, Json<ErrorResponse>) {
    (
        StatusCode::UNAUTHORIZED,
        Json(ErrorResponse {
            error: message.into(),
        }),
    )
}

fn dependency_unavailable(message: impl Into<String>) -> (StatusCode, Json<ErrorResponse>) {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(ErrorResponse {
            error: message.into(),
        }),
    )
}
