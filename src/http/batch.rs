use crate::app_state::AppState;
use crate::error::AppError;
use crate::infra::metrics::record_cache_operation;
use crate::integrations::profile_api_client::AuthenticatedUser;
use crate::storage::likes_repository::PostgresLikesRepository;
use axum::{
    extract::{Extension, State},
    response::{IntoResponse, Response},
    Json,
};
use std::collections::HashMap;

use super::dto::{
    BatchLikeCountItemResponse, BatchLikeCountsResponse, BatchLikeStatusItemResponse,
    BatchLikeStatusesResponse, BatchLikesRequest,
};
use super::helpers::{
    cache_like_count, get_cached_like_counts, like_count_cache_key, parse_authenticated_user_id,
    parse_batch_items, success, MAX_BATCH_ITEMS,
};

pub(crate) async fn get_like_counts_batch(
    State(state): State<AppState>,
    Json(payload): Json<BatchLikesRequest>,
) -> Response {
    if payload.items.len() > MAX_BATCH_ITEMS {
        return AppError::invalid_request("BATCH_TOO_LARGE", "batch too large").into_response();
    }

    let parsed_items = match parse_batch_items(&payload.items) {
        Ok(items) => items,
        Err(error) => return error.into_response(),
    };

    let cache_keys: Vec<String> = parsed_items
        .iter()
        .map(|(content_type, content_id)| like_count_cache_key(content_type, content_id))
        .collect();

    let cached_counts = match get_cached_like_counts(&state, &cache_keys).await {
        Ok(values) => values,
        Err(error) => {
            record_cache_operation("get_like_counts_batch", "error");
            tracing::warn!(
                service = "likes_service",
                error = %error,
                "redis unavailable for get_like_counts_batch, falling back to postgres"
            );
            vec![None; parsed_items.len()]
        }
    };

    let mut counts_by_item: HashMap<(String, String), i64> = HashMap::new();
    let mut missing_items = Vec::new();

    for ((content_type, content_id), cached_count) in parsed_items.iter().zip(cached_counts) {
        let key = (content_type.to_string(), content_id.to_string());

        if let Some(count) = cached_count {
            record_cache_operation("get_like_counts_batch", "hit");
            counts_by_item.insert(key, count);
        } else {
            record_cache_operation("get_like_counts_batch", "miss");
            missing_items.push((content_type.clone(), content_id.clone()));
        }
    }

    if !missing_items.is_empty() {
        let repository = PostgresLikesRepository::new(&state.db_pool);
        let postgres_counts = match repository.get_like_counts_batch(&missing_items).await {
            Ok(values) => values,
            Err(error) => return error.into_response(),
        };

        for (content_type, content_id) in &missing_items {
            let key = (content_type.to_string(), content_id.to_string());
            let count = *postgres_counts.get(&key).unwrap_or(&0);

            counts_by_item.insert(key.clone(), count);

            if let Err(error) =
                cache_like_count(&state, &like_count_cache_key(content_type, content_id), count)
                    .await
            {
                record_cache_operation("get_like_counts_batch", "error");
                tracing::warn!(
                    service = "likes_service",
                    error = %error,
                    "failed to populate redis cache for get_like_counts_batch"
                );
            }
        }
    }

    let items = parsed_items
        .into_iter()
        .map(|(content_type, content_id)| BatchLikeCountItemResponse {
            content_type: content_type.to_string(),
            content_id: content_id.to_string(),
            count: *counts_by_item
                .get(&(content_type.to_string(), content_id.to_string()))
                .unwrap_or(&0),
        })
        .collect();

    success(Json(BatchLikeCountsResponse { results: items })).into_response()
}

pub(crate) async fn get_like_statuses_batch(
    State(state): State<AppState>,
    Extension(authenticated_user): Extension<AuthenticatedUser>,
    Json(payload): Json<BatchLikesRequest>,
) -> Response {
    if payload.items.len() > MAX_BATCH_ITEMS {
        return AppError::invalid_request("BATCH_TOO_LARGE", "batch too large").into_response();
    }

    let user_id = match parse_authenticated_user_id(&authenticated_user) {
        Ok(value) => value,
        Err(error) => return error.into_response(),
    };

    let parsed_items = match parse_batch_items(&payload.items) {
        Ok(items) => items,
        Err(error) => return error.into_response(),
    };

    let repository = PostgresLikesRepository::new(&state.db_pool);
    let statuses = match repository
        .get_like_statuses_batch(&user_id, &parsed_items)
        .await
    {
        Ok(values) => values,
        Err(error) => return error.into_response(),
    };

    let items = parsed_items
        .into_iter()
        .map(|(content_type, content_id)| {
            let key = (content_type.to_string(), content_id.to_string());
            let status = statuses.get(&key);

            BatchLikeStatusItemResponse {
                content_type: key.0,
                content_id: key.1,
                liked: status.map(|value| value.exists).unwrap_or(false),
                liked_at: status.and_then(|value| value.liked_at.clone()),
            }
        })
        .collect();

    success(Json(BatchLikeStatusesResponse { results: items })).into_response()
}
