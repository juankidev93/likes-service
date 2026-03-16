use crate::app_state::AppState;
use crate::error::AppError;
use crate::profile_api_client::AuthError;
use axum::{
    extract::Request,
    http::{header, HeaderMap},
    middleware::Next,
    response::{IntoResponse, Response},
};

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
        Err(AuthError::InvalidToken) => {
            AppError::unauthorized("UNAUTHORIZED", "invalid bearer token").into_response()
        }
        Err(AuthError::DependencyUnavailable(_)) | Err(AuthError::NetworkError(_)) => {
            AppError::dependency_unavailable("DEPENDENCY_UNAVAILABLE", "dependency unavailable")
                .into_response()
        }
    }
}

fn extract_bearer_token(headers: &HeaderMap) -> Result<&str, Response> {
    let authorization = headers
        .get(header::AUTHORIZATION)
        .ok_or_else(|| {
            AppError::unauthorized("UNAUTHORIZED", "missing authorization header").into_response()
        })?;

    let authorization = authorization
        .to_str()
        .map_err(|_| {
            AppError::unauthorized("UNAUTHORIZED", "invalid authorization header").into_response()
        })?;

    authorization
        .strip_prefix("Bearer ")
        .filter(|token| !token.trim().is_empty())
        .ok_or_else(|| {
            AppError::unauthorized("UNAUTHORIZED", "invalid authorization header").into_response()
        })
}
