use crate::app_state::AppState;
use crate::error::AppError;
use crate::infra::logging::LoggedUserId;
use crate::integrations::profile_api_client::{AuthError, AuthenticatedUser};
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
    match authenticate_headers(&state, request.headers()).await {
        Ok(authenticated_user) => {
            request.extensions_mut().insert(authenticated_user.clone());
            let mut response = next.run(request).await;
            response
                .extensions_mut()
                .insert(LoggedUserId(authenticated_user.user_id));
            response
        }
        Err(response) => response,
    }
}

pub async fn authenticate_headers(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<AuthenticatedUser, Response> {
    let bearer_token = extract_bearer_token(headers)?;

    match state.profile_api_client.validate_token(bearer_token).await {
        Ok(authenticated_user) => Ok(authenticated_user),
        Err(AuthError::InvalidToken) => {
            Err(AppError::unauthorized("UNAUTHORIZED", "invalid bearer token").into_response())
        }
        Err(AuthError::DependencyUnavailable(_)) | Err(AuthError::NetworkError(_)) => Err(
            AppError::dependency_unavailable("DEPENDENCY_UNAVAILABLE", "dependency unavailable")
                .into_response(),
        ),
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
