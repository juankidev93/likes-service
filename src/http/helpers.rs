use crate::app_state::AppState;
use crate::domain::{ContentId, ContentType, UserId};
use crate::error::AppError;
use crate::likes_repository::{LikesCursor, TopLikesWindow, UserLikeRow};
use crate::profile_api_client::AuthenticatedUser;
use axum::{http::StatusCode, Json};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use redis::AsyncCommands;
use std::str::FromStr;

use super::dto::BatchLikeItemRequest;

const DEFAULT_USER_LIKES_LIMIT: usize = 20;
pub(super) const MAX_BATCH_ITEMS: usize = 100;
const DEFAULT_TOP_LIKES_LIMIT: usize = 10;
const MAX_TOP_LIKES_LIMIT: usize = 50;
const LIKE_COUNT_CACHE_TTL_SECONDS: u64 = 60;

pub(super) fn parse_authenticated_user_id(
    authenticated_user: &AuthenticatedUser,
) -> Result<UserId, AppError> {
    UserId::from_str(&authenticated_user.user_id).map_err(AppError::from)
}

pub(super) fn success<T>(payload: Json<T>) -> (StatusCode, Json<T>) {
    (StatusCode::OK, payload)
}

pub(super) fn like_count_cache_key(content_type: &ContentType, content_id: &ContentId) -> String {
    format!("likes:count:{content_type}:{content_id}")
}

pub(super) fn parse_limit(limit: Option<usize>) -> Result<usize, AppError> {
    let limit = limit.unwrap_or(DEFAULT_USER_LIKES_LIMIT);

    if limit == 0 || limit > MAX_BATCH_ITEMS {
        return Err(AppError::invalid_request(
            "INVALID_REQUEST",
            format!("limit must be between 1 and {MAX_BATCH_ITEMS}"),
        ));
    }

    Ok(limit)
}

pub(super) fn parse_top_likes_limit(limit: Option<usize>) -> Result<usize, AppError> {
    let limit = limit.unwrap_or(DEFAULT_TOP_LIKES_LIMIT);

    if limit == 0 || limit > MAX_TOP_LIKES_LIMIT {
        return Err(AppError::invalid_request(
            "INVALID_REQUEST",
            format!("limit must be between 1 and {MAX_TOP_LIKES_LIMIT}"),
        ));
    }

    Ok(limit)
}

pub(super) fn parse_top_likes_window(window: Option<&str>) -> Result<TopLikesWindow, AppError> {
    TopLikesWindow::from_str(window.unwrap_or("24h"))
}

pub(super) fn encode_cursor(row: &UserLikeRow) -> String {
    let raw = format!("{}|{}", row.liked_at, row.content_id);
    STANDARD.encode(raw)
}

pub(super) fn decode_cursor(value: &str) -> Result<LikesCursor, AppError> {
    let decoded = STANDARD
        .decode(value)
        .map_err(|_| AppError::invalid_request("INVALID_REQUEST", "invalid cursor"))?;

    let decoded = String::from_utf8(decoded)
        .map_err(|_| AppError::invalid_request("INVALID_REQUEST", "invalid cursor"))?;

    let (liked_at, content_id) = decoded
        .split_once('|')
        .ok_or_else(|| AppError::invalid_request("INVALID_REQUEST", "invalid cursor"))?;

    if liked_at.is_empty() || content_id.is_empty() {
        return Err(AppError::invalid_request("INVALID_REQUEST", "invalid cursor"));
    }

    Ok(LikesCursor {
        liked_at: liked_at.to_string(),
        content_id: content_id.to_string(),
    })
}

pub(super) async fn get_cached_like_count(state: &AppState, key: &str) -> Result<Option<i64>, AppError> {
    let mut redis_connection = state.redis_client.get_multiplexed_async_connection().await?;
    let count = redis_connection.get(key).await?;
    Ok(count)
}

pub(super) async fn get_cached_like_counts(
    state: &AppState,
    keys: &[String],
) -> Result<Vec<Option<i64>>, AppError> {
    if keys.is_empty() {
        return Ok(Vec::new());
    }

    let mut redis_connection = state.redis_client.get_multiplexed_async_connection().await?;
    let counts = redis::cmd("MGET")
        .arg(keys)
        .query_async(&mut redis_connection)
        .await?;

    Ok(counts)
}

pub(super) async fn cache_like_count(
    state: &AppState,
    key: &str,
    count: i64,
) -> Result<(), AppError> {
    let mut redis_connection = state.redis_client.get_multiplexed_async_connection().await?;
    let _: () = redis_connection
        .set_ex(key, count, LIKE_COUNT_CACHE_TTL_SECONDS)
        .await?;
    Ok(())
}

pub(super) fn parse_batch_items(
    items: &[BatchLikeItemRequest],
) -> Result<Vec<(ContentType, ContentId)>, AppError> {
    items
        .iter()
        .map(|item| {
            let content_type = ContentType::from_str(&item.content_type).map_err(AppError::from)?;
            let content_id = ContentId::from_str(&item.content_id).map_err(AppError::from)?;

            Ok((content_type, content_id))
        })
        .collect()
}
