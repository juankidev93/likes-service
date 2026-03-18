use crate::app_state::AppState;
use crate::domain::ContentType;
use crate::error::AppError;
use crate::storage::likes_repository::PostgresLikesRepository;
use axum::{
    extract::{Query, State},
    response::{IntoResponse, Response},
    Json,
};
use std::str::FromStr;

use super::dto::{TopLikeItemResponse, TopLikesQuery, TopLikesResponse};
use super::helpers::{
    cache_top_likes, get_cached_top_likes, parse_top_likes_limit, parse_top_likes_window, success,
    top_likes_cache_key,
};

pub(crate) async fn get_top_likes(
    State(state): State<AppState>,
    Query(query): Query<TopLikesQuery>,
) -> Response {
    match build_top_likes_response(&state, query).await {
        Ok(response) => success(Json(response)).into_response(),
        Err(error) => error.into_response(),
    }
}

pub(crate) async fn build_top_likes_response(
    state: &AppState,
    query: TopLikesQuery,
) -> Result<TopLikesResponse, AppError> {
    let window = parse_top_likes_window(query.window.as_deref())?;
    let limit = parse_top_likes_limit(query.limit)?;
    let content_type = parse_top_likes_content_type(query.content_type.as_deref())?;
    let cache_key = top_likes_cache_key(&window, content_type.as_ref(), limit);

    match get_cached_top_likes(state, &cache_key).await {
        Ok(Some(response)) => return Ok(response),
        Ok(None) => {}
        Err(error) => {
            tracing::warn!(
                service = "likes_service",
                error = %error,
                "redis unavailable for top likes cache, falling back to postgres"
            );
        }
    }

    let repository = PostgresLikesRepository::new(&state.read_db_pool);
    let rows = repository
        .list_top_likes(content_type.as_ref(), &window, limit)
        .await?;

    let response = TopLikesResponse {
        window: window.as_str().to_string(),
        content_type: content_type.as_ref().map(ToString::to_string),
        items: rows.into_iter().map(map_top_like_row).collect(),
    };

    if let Err(error) = cache_top_likes(state, &cache_key, &response).await {
        tracing::warn!(
            service = "likes_service",
            error = %error,
            "failed to populate top likes cache"
        );
    }

    Ok(response)
}

fn parse_top_likes_content_type(
    content_type: Option<&str>,
) -> Result<Option<ContentType>, AppError> {
    match content_type {
        Some(value) => match ContentType::from_str(value) {
            Ok(content_type) => Ok(Some(content_type)),
            Err(error) => Err(AppError::from(error)),
        },
        None => Ok(None),
    }
}

fn map_top_like_row(row: crate::storage::likes_repository::TopLikeRow) -> TopLikeItemResponse {
    TopLikeItemResponse {
        content_type: row.content_type,
        content_id: row.content_id,
        count: row.like_count,
    }
}
