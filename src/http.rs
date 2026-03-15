use crate::app_state::AppState;
use crate::domain::{ContentId, ContentType, UserId};
use crate::error::AppError;
use crate::likes_repository::PostgresLikesRepository;
use crate::use_cases::{LikeContentResult, LikesUseCases, UnlikeContentResult};
use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use std::str::FromStr;

pub async fn create_like(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<CreateLikeRequest>,
) -> Response {
    let user_id = match parse_user_id(&headers) {
        Ok(value) => value,
        Err(response) => return response.into_response(),
    };

    let content_type = match ContentType::from_str(&payload.content_type) {
        Ok(value) => value,
        Err(error) => return bad_request(error.to_string()).into_response(),
    };

    let content_id = match ContentId::from_str(&payload.content_id) {
        Ok(value) => value,
        Err(error) => return bad_request(error.to_string()).into_response(),
    };

    let repository = PostgresLikesRepository::new(&state.db_pool);
    let use_cases = LikesUseCases::new(repository);

    match use_cases
        .like_content(&user_id, &content_type, &content_id)
        .await
    {
        Ok(result) => success(Json(LikeResponse::from(result))).into_response(),
        Err(error) => internal_error(error).into_response(),
    }
}

pub async fn delete_like(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((content_type, content_id)): Path<(String, String)>,
) -> Response {
    let user_id = match parse_user_id(&headers) {
        Ok(value) => value,
        Err(response) => return response.into_response(),
    };

    let content_type = match ContentType::from_str(&content_type) {
        Ok(value) => value,
        Err(error) => return bad_request(error.to_string()).into_response(),
    };

    let content_id = match ContentId::from_str(&content_id) {
        Ok(value) => value,
        Err(error) => return bad_request(error.to_string()).into_response(),
    };

    let repository = PostgresLikesRepository::new(&state.db_pool);
    let use_cases = LikesUseCases::new(repository);

    match use_cases
        .unlike_content(&user_id, &content_type, &content_id)
        .await
    {
        Ok(result) => success(Json(UnlikeResponse::from(result))).into_response(),
        Err(error) => internal_error(error).into_response(),
    }
}

#[derive(Deserialize)]
pub struct CreateLikeRequest {
    pub content_type: String,
    pub content_id: String,
}

#[derive(Serialize)]
pub struct LikeResponse {
    pub result: &'static str,
}

impl From<LikeContentResult> for LikeResponse {
    fn from(value: LikeContentResult) -> Self {
        match value {
            LikeContentResult::Liked => Self { result: "liked" },
            LikeContentResult::AlreadyLiked => Self {
                result: "already_liked",
            },
        }
    }
}

#[derive(Serialize)]
pub struct UnlikeResponse {
    pub result: &'static str,
}

impl From<UnlikeContentResult> for UnlikeResponse {
    fn from(value: UnlikeContentResult) -> Self {
        match value {
            UnlikeContentResult::Unliked => Self { result: "unliked" },
            UnlikeContentResult::NotLiked => Self {
                result: "not_liked",
            },
        }
    }
}

#[derive(Serialize)]
struct ErrorResponse {
    error: String,
}

fn parse_user_id(headers: &HeaderMap) -> Result<UserId, (StatusCode, Json<ErrorResponse>)> {
    let header_value = headers
        .get("x-user-id")
        .ok_or_else(|| bad_request("missing x-user-id header".to_string()))?;

    let user_id = header_value
        .to_str()
        .map_err(|_| bad_request("x-user-id header must be valid UTF-8".to_string()))?;

    UserId::from_str(user_id).map_err(|error| bad_request(error.to_string()))
}

fn success<T>(payload: Json<T>) -> (StatusCode, Json<T>) {
    (StatusCode::OK, payload)
}

fn bad_request(message: String) -> (StatusCode, Json<ErrorResponse>) {
    (
        StatusCode::BAD_REQUEST,
        Json(ErrorResponse { error: message }),
    )
}

fn internal_error(error: AppError) -> (StatusCode, Json<ErrorResponse>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ErrorResponse {
            error: error.to_string(),
        }),
    )
}
