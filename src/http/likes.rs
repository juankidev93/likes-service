use crate::app_state::{AppState, LikeCountFillPermit};
use crate::domain::{ContentId, ContentType};
use crate::error::AppError;
use crate::infra::metrics::record_cache_operation;
use crate::integrations::profile_api_client::AuthenticatedUser;
use crate::storage::likes_repository::PostgresLikesRepository;
use crate::use_cases::LikesUseCases;
use axum::{
    Json,
    extract::{Extension, Path, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use std::str::FromStr;

use super::dto::{
    CreateLikeRequest, LikeCountResponse, LikeResponse, LikeStatusResponse, UnlikeResponse,
};
use super::helpers::{
    cache_control_for_count, cache_like_count, cache_like_status, count_response_etag,
    get_cached_like_count, get_cached_like_status, if_none_match_matches, like_count_cache_key,
    parse_authenticated_user_id, publish_like_count_update, store_local_like_count, success,
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
            let cache_key = like_count_cache_key(&content_type, &content_id);
            if let Err(error) = cache_like_count(&state, &cache_key, result.count).await {
                tracing::warn!(
                    service = "likes_service",
                    error = %error,
                    "failed to update redis cache for like"
                );
                store_local_like_count(&state, &cache_key, result.count);
            }

            if let Err(error) =
                publish_like_count_update(&state, &content_type, &content_id, result.count).await
            {
                tracing::warn!(
                    service = "likes_service",
                    error = %error,
                    "failed to publish count cache update for like"
                );
            }

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

            let status_response = LikeStatusResponse {
                liked: true,
                liked_at: result.liked_at.clone(),
            };
            if let Err(error) = cache_like_status(
                &state,
                &user_id,
                &content_type,
                &content_id,
                &status_response,
            )
            .await
            {
                tracing::warn!(
                    service = "likes_service",
                    error = %error,
                    "failed to update cached like status for like"
                );
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
            let cache_key = like_count_cache_key(&content_type, &content_id);
            if let Err(error) = cache_like_count(&state, &cache_key, result.count).await {
                tracing::warn!(
                    service = "likes_service",
                    error = %error,
                    "failed to update redis cache for unlike"
                );
                store_local_like_count(&state, &cache_key, result.count);
            }

            if let Err(error) =
                publish_like_count_update(&state, &content_type, &content_id, result.count).await
            {
                tracing::warn!(
                    service = "likes_service",
                    error = %error,
                    "failed to publish count cache update for unlike"
                );
            }

            if result.was_liked {
                if let Err(error) = state
                    .like_events
                    .publish_unlike(&user_id, &content_type, &content_id, result.count)
                    .await
                {
                    tracing::warn!(service = "likes_service", error = %error, "failed to publish unlike event");
                }
            }

            let status_response = LikeStatusResponse {
                liked: false,
                liked_at: None,
            };
            if let Err(error) = cache_like_status(
                &state,
                &user_id,
                &content_type,
                &content_id,
                &status_response,
            )
            .await
            {
                tracing::warn!(
                    service = "likes_service",
                    error = %error,
                    "failed to update cached like status for unlike"
                );
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

    match get_cached_like_status(&state, &user_id, &content_type, &content_id).await {
        Ok(Some(status)) => return success(Json(status)).into_response(),
        Ok(None) => {}
        Err(error) => {
            tracing::warn!(
                service = "likes_service",
                error = %error,
                "redis unavailable for get_like_status, falling back to postgres"
            );
        }
    }

    match repository
        .get_like_status(&user_id, &content_type, &content_id)
        .await
    {
        Ok(status) => {
            let response = LikeStatusResponse::from(status);
            if let Err(error) = cache_like_status(&state, &user_id, &content_type, &content_id, &response).await {
                tracing::warn!(
                    service = "likes_service",
                    error = %error,
                    "failed to populate cached like status"
                );
            }
            success(Json(response)).into_response()
        }
        Err(error) => error.into_response(),
    }
}

pub(crate) async fn get_like_count(
    State(state): State<AppState>,
    headers: HeaderMap,
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
    let if_none_match = headers
        .get(header::IF_NONE_MATCH)
        .and_then(|value| value.to_str().ok());

    match get_cached_like_count(&state, &cache_key).await {
        Ok(Some(count)) => {
            record_cache_operation("get_like_count", "hit");
            return build_count_response(&content_type, &content_id, count, if_none_match);
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

    loop {
        match state.begin_like_count_fill(&cache_key).await {
            LikeCountFillPermit::Leader(notify) => {
                let result = repository.get_like_count(&content_type, &content_id).await;
                state.finish_like_count_fill(&cache_key, &notify).await;

                match result {
                    Ok(count) => {
                        if let Err(error) = cache_like_count(&state, &cache_key, count.count).await {
                            record_cache_operation("get_like_count", "error");
                            tracing::warn!(service = "likes_service", error = %error, "failed to populate redis cache for get_like_count");
                        }

                        return build_count_response(&content_type, &content_id, count.count, if_none_match);
                    }
                    Err(error) => return error.into_response(),
                }
            }
            LikeCountFillPermit::Follower(notify) => {
                notify.notified().await;

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
                        continue;
                    }
                    Err(error) => {
                        record_cache_operation("get_like_count", "error");
                        tracing::warn!(
                            service = "likes_service",
                            error = %error,
                            "redis unavailable after waiting for get_like_count fill, falling back to leader path"
                        );
                    }
                }
            }
        }
    }
}

fn build_count_response(
    content_type: &ContentType,
    content_id: &ContentId,
    count: i64,
    if_none_match: Option<&str>,
) -> Response {
    let etag = count_response_etag(content_type, content_id, count);

    if if_none_match_matches(if_none_match, &etag) {
        let mut response = StatusCode::NOT_MODIFIED.into_response();
        response.headers_mut().insert(
            header::ETAG,
            HeaderValue::from_str(&etag).expect("etag header must be valid"),
        );
        response.headers_mut().insert(
            header::CACHE_CONTROL,
            HeaderValue::from_static(cache_control_for_count()),
        );
        return response;
    }

    let mut response = success(Json(LikeCountResponse::from_parts(
        content_type,
        content_id,
        count,
    )))
    .into_response();
    response.headers_mut().insert(
        header::ETAG,
        HeaderValue::from_str(&etag).expect("etag header must be valid"),
    );
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static(cache_control_for_count()),
    );
    response
}
