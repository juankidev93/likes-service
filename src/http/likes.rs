use crate::app_state::AppState;
use crate::domain::{ContentId, ContentType};
use crate::error::AppError;
use crate::infra::metrics::record_cache_operation;
use crate::integrations::profile_api_client::AuthenticatedUser;
use crate::storage::likes_repository::PostgresLikesRepository;
use crate::use_cases::LikesUseCases;
use axum::{
    extract::{Extension, Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use std::str::FromStr;

use super::dto::{CreateLikeRequest, LikeCountResponse, LikeResponse, LikeStatusResponse, UnlikeResponse};
use super::helpers::{
    cache_like_count, get_cached_like_count, like_count_cache_key, parse_authenticated_user_id,
    success,
};

pub(crate) async fn create_like(
    State(state): State<AppState>,
    Extension(authenticated_user): Extension<AuthenticatedUser>,
    Json(payload): Json<CreateLikeRequest>,
) -> Response {
    let user_id = match parse_authenticated_user_id(&authenticated_user) {
        Ok(value) => value,
        Err(error) => return error.into_response(),
    };

    let content_type = match ContentType::from_str(&payload.content_type) {
        Ok(value) => value,
        Err(error) => return AppError::from(error).into_response(),
    };

    let content_id = match ContentId::from_str(&payload.content_id) {
        Ok(value) => value,
        Err(error) => return AppError::from(error).into_response(),
    };

    let repository = PostgresLikesRepository::new(&state.db_pool);
    let use_cases = LikesUseCases::new(
        repository,
        state.redis_client.clone(),
        state.content_validation_client.clone(),
        state.cache_ttl_like_counts_seconds,
    );

    match use_cases
        .like_content(&user_id, &content_type, &content_id)
        .await
    {
        Ok(result) => {
            if !result.already_existed {
                if let Err(error) = state
                    .like_events
                    .publish_like(
                        &user_id,
                        &content_type,
                        &content_id,
                        result.count,
                        result.liked_at.as_deref(),
                    )
                    .await
                {
                    tracing::warn!(service = "likes_service", error = %error, "failed to publish like event");
                }
            }

            let status = if result.already_existed {
                StatusCode::OK
            } else {
                StatusCode::CREATED
            };

            (status, Json(LikeResponse::from(result))).into_response()
        }
        Err(error) => error.into_response(),
    }
}

pub(crate) async fn delete_like(
    State(state): State<AppState>,
    Extension(authenticated_user): Extension<AuthenticatedUser>,
    Path((content_type, content_id)): Path<(String, String)>,
) -> Response {
    let user_id = match parse_authenticated_user_id(&authenticated_user) {
        Ok(value) => value,
        Err(error) => return error.into_response(),
    };

    let content_type = match ContentType::from_str(&content_type) {
        Ok(value) => value,
        Err(error) => return AppError::from(error).into_response(),
    };

    let content_id = match ContentId::from_str(&content_id) {
        Ok(value) => value,
        Err(error) => return AppError::from(error).into_response(),
    };

    let repository = PostgresLikesRepository::new(&state.db_pool);
    let use_cases = LikesUseCases::new(
        repository,
        state.redis_client.clone(),
        state.content_validation_client.clone(),
        state.cache_ttl_like_counts_seconds,
    );

    match use_cases
        .unlike_content(&user_id, &content_type, &content_id)
        .await
    {
        Ok(result) => {
            if result.was_liked {
                if let Err(error) = state
                    .like_events
                    .publish_unlike(
                        &user_id,
                        &content_type,
                        &content_id,
                        result.count,
                    )
                    .await
                {
                    tracing::warn!(service = "likes_service", error = %error, "failed to publish unlike event");
                }
            }

            success(Json(UnlikeResponse::from(result))).into_response()
        }
        Err(error) => error.into_response(),
    }
}

pub(crate) async fn get_like_status(
    State(state): State<AppState>,
    Extension(authenticated_user): Extension<AuthenticatedUser>,
    Path((content_type, content_id)): Path<(String, String)>,
) -> Response {
    let user_id = match parse_authenticated_user_id(&authenticated_user) {
        Ok(value) => value,
        Err(error) => return error.into_response(),
    };

    let content_type = match ContentType::from_str(&content_type) {
        Ok(value) => value,
        Err(error) => return AppError::from(error).into_response(),
    };

    let content_id = match ContentId::from_str(&content_id) {
        Ok(value) => value,
        Err(error) => return AppError::from(error).into_response(),
    };

    let repository = PostgresLikesRepository::new(&state.read_db_pool);

    match repository
        .get_like_status(&user_id, &content_type, &content_id)
        .await
    {
        Ok(status) => success(Json(LikeStatusResponse::from(status))).into_response(),
        Err(error) => error.into_response(),
    }
}

pub(crate) async fn get_like_count(
    State(state): State<AppState>,
    Path((content_type, content_id)): Path<(String, String)>,
) -> Response {
    let content_type = match ContentType::from_str(&content_type) {
        Ok(value) => value,
        Err(error) => return AppError::from(error).into_response(),
    };

    let content_id = match ContentId::from_str(&content_id) {
        Ok(value) => value,
        Err(error) => return AppError::from(error).into_response(),
    };

    let repository = PostgresLikesRepository::new(&state.read_db_pool);
    let cache_key = like_count_cache_key(&content_type, &content_id);

    match get_cached_like_count(&state, &cache_key).await {
        Ok(Some(count)) => {
            record_cache_operation("get_like_count", "hit");
            return success(Json(LikeCountResponse {
                content_type: content_type.to_string(),
                content_id: content_id.to_string(),
                count,
            }))
            .into_response();
        }
        Ok(None) => {
            record_cache_operation("get_like_count", "miss");
        }
        Err(error) => {
            record_cache_operation("get_like_count", "error");
            tracing::warn!(
                service = "likes_service",
                error = %error,
                "redis unavailable for get_like_count, falling back to postgres"
            );
        }
    }

    match repository.get_like_count(&content_type, &content_id).await {
        Ok(count) => {
            if let Err(error) = cache_like_count(&state, &cache_key, count.count).await {
                record_cache_operation("get_like_count", "error");
                tracing::warn!(service = "likes_service", error = %error, "failed to populate redis cache for get_like_count");
            }

            success(Json(LikeCountResponse::from_parts(
                &content_type,
                &content_id,
                count.count,
            )))
            .into_response()
        }
        Err(error) => error.into_response(),
    }
}
