use crate::app_state::{AppState, LikeCountCacheUpdate};
use crate::domain::{ContentId, ContentType, UserId};
use crate::error::AppError;
use crate::integrations::profile_api_client::AuthenticatedUser;
use crate::storage::likes_repository::{LikesCursor, TopLikesWindow, UserLikeRow};
use axum::{http::StatusCode, Json};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use redis::AsyncCommands;
use serde::{Serialize, de::DeserializeOwned};
use std::str::FromStr;
use std::time::Duration;

use super::dto::{BatchLikeItemRequest, LikeStatusResponse, TopLikesResponse};

const DEFAULT_USER_LIKES_LIMIT: usize = 20;
pub(super) const MAX_BATCH_ITEMS: usize = 100;
const DEFAULT_TOP_LIKES_LIMIT: usize = 10;
const MAX_TOP_LIKES_LIMIT: usize = 50;
const LOCAL_LIKE_COUNT_CACHE_TTL_MS: u64 = 5000;
pub(crate) const LIKE_COUNT_UPDATES_CHANNEL: &str = "likes:count_updates";

pub(super) fn parse_authenticated_user_id(
    authenticated_user: &AuthenticatedUser,
) -> Result<UserId, AppError> {
    UserId::from_str(&authenticated_user.user_id).map_err(AppError::from)
}

pub(super) fn success<T>(payload: Json<T>) -> (StatusCode, Json<T>) {
    (StatusCode::OK, payload)
}

pub(crate) fn cache_control_for_count() -> &'static str {
    "public, max-age=5, stale-while-revalidate=55"
}

pub(crate) fn cache_control_for_top_likes() -> &'static str {
    "public, max-age=30, stale-while-revalidate=30"
}

pub(crate) fn count_response_etag(
    content_type: &ContentType,
    content_id: &ContentId,
    count: i64,
) -> String {
    format!("\"count:{}:{}:{}\"", content_type, content_id, count)
}

pub(crate) fn top_likes_response_etag(response: &TopLikesResponse) -> String {
    let mut raw = format!(
        "top:{}:{}",
        response.window,
        response.content_type.as_deref().unwrap_or("all")
    );

    for item in &response.items {
        raw.push('|');
        raw.push_str(&item.content_type);
        raw.push(':');
        raw.push_str(&item.content_id);
        raw.push(':');
        raw.push_str(&item.count.to_string());
    }

    format!("\"{}\"", STANDARD.encode(raw))
}

pub(crate) fn if_none_match_matches(header_value: Option<&str>, etag: &str) -> bool {
    let Some(header_value) = header_value else {
        return false;
    };

    header_value
        .split(',')
        .map(str::trim)
        .any(|candidate| candidate == "*" || candidate == etag)
}

pub(super) fn like_count_cache_key(content_type: &ContentType, content_id: &ContentId) -> String {
    format!("likes:count:{content_type}:{content_id}")
}

pub(super) fn like_status_cache_key(
    user_id: &UserId,
    content_type: &ContentType,
    content_id: &ContentId,
) -> String {
    format!("likes:status:{user_id}:{content_type}:{content_id}")
}

pub(super) fn top_likes_cache_key(
    window: &TopLikesWindow,
    content_type: Option<&ContentType>,
    limit: usize,
) -> String {
    let content_type = content_type
        .map(ToString::to_string)
        .unwrap_or_else(|| "all".to_string());
    format!("likes:top:{}:{}:{}", window.as_str(), content_type, limit)
}

pub(crate) fn parse_limit(limit: Option<usize>) -> Result<usize, AppError> {
    let limit = limit.unwrap_or(DEFAULT_USER_LIKES_LIMIT);

    if limit == 0 || limit > MAX_BATCH_ITEMS {
        return Err(AppError::invalid_request(
            "INVALID_LIMIT",
            format!("limit must be between 1 and {MAX_BATCH_ITEMS}"),
        ));
    }

    Ok(limit)
}

pub(crate) fn parse_top_likes_limit(limit: Option<usize>) -> Result<usize, AppError> {
    let limit = limit.unwrap_or(DEFAULT_TOP_LIKES_LIMIT);

    if limit == 0 || limit > MAX_TOP_LIKES_LIMIT {
        return Err(AppError::invalid_request(
            "INVALID_LIMIT",
            format!("limit must be between 1 and {MAX_TOP_LIKES_LIMIT}"),
        ));
    }

    Ok(limit)
}

pub(crate) fn parse_top_likes_window(window: Option<&str>) -> Result<TopLikesWindow, AppError> {
    TopLikesWindow::from_str(window.unwrap_or("24h"))
}

pub(crate) fn encode_cursor(row: &UserLikeRow) -> String {
    let raw = format!("{}|{}", row.liked_at, row.content_id);
    STANDARD.encode(raw)
}

pub(crate) fn decode_cursor(value: &str) -> Result<LikesCursor, AppError> {
    let decoded = STANDARD
        .decode(value)
        .map_err(|_| AppError::invalid_request("INVALID_CURSOR", "invalid cursor"))?;

    let decoded = String::from_utf8(decoded)
        .map_err(|_| AppError::invalid_request("INVALID_CURSOR", "invalid cursor"))?;

    let (liked_at, content_id) = decoded
        .split_once('|')
        .ok_or_else(|| AppError::invalid_request("INVALID_CURSOR", "invalid cursor"))?;

    if liked_at.is_empty() || content_id.is_empty() {
        return Err(AppError::invalid_request(
            "INVALID_CURSOR",
            "invalid cursor",
        ));
    }

    Ok(LikesCursor {
        liked_at: liked_at.to_string(),
        content_id: content_id.to_string(),
    })
}

pub(super) async fn get_cached_like_count(
    state: &AppState,
    key: &str,
) -> Result<Option<i64>, AppError> {
    if let Some(count) = state.local_like_count_cache.get(key) {
        return Ok(Some(count));
    }

    let mut redis_connection = state.get_redis_connection().await?;
    let count = redis_connection.get(key).await?;

    if let Some(count) = count {
        store_local_like_count(state, key, count);
    }

    Ok(count)
}

pub(super) async fn get_cached_like_counts(
    state: &AppState,
    keys: &[String],
) -> Result<Vec<Option<i64>>, AppError> {
    if keys.is_empty() {
        return Ok(Vec::new());
    }

    let mut counts = vec![None; keys.len()];
    let mut missing_indexes = Vec::new();
    let mut missing_keys = Vec::new();

    for (index, key) in keys.iter().enumerate() {
        if let Some(count) = state.local_like_count_cache.get(key) {
            counts[index] = Some(count);
        } else {
            missing_indexes.push(index);
            missing_keys.push(key.clone());
        }
    }

    if missing_keys.is_empty() {
        return Ok(counts);
    }

    let mut redis_connection = state.get_redis_connection().await?;
    let redis_counts: Vec<Option<i64>> = redis::cmd("MGET")
        .arg(&missing_keys)
        .query_async(&mut redis_connection)
        .await?;

    for (index, count) in missing_indexes.into_iter().zip(redis_counts.into_iter()) {
        if let Some(count) = count {
            store_local_like_count(state, &keys[index], count);
            counts[index] = Some(count);
        } else {
            counts[index] = None;
        }
    }

    Ok(counts)
}

pub(super) async fn cache_like_count(
    state: &AppState,
    key: &str,
    count: i64,
) -> Result<(), AppError> {
    store_local_like_count(state, key, count);

    let mut redis_connection = state.get_redis_connection().await?;
    let _: () = redis_connection
        .set_ex(key, count, state.cache_ttl_like_counts_seconds)
        .await?;
    Ok(())
}

pub(super) async fn publish_like_count_update(
    state: &AppState,
    content_type: &ContentType,
    content_id: &ContentId,
    count: i64,
) -> Result<(), AppError> {
    let payload = serde_json::to_string(&LikeCountCacheUpdate {
        content_type: content_type.to_string(),
        content_id: content_id.to_string(),
        count,
    })
    .map_err(|error| {
        AppError::dependency_unavailable(
            "CACHE_SERIALIZATION_ERROR",
            format!("failed to encode count cache update: {error}"),
        )
    })?;

    let mut redis_connection = state.get_redis_connection().await?;
    let _: i64 = redis::cmd("PUBLISH")
        .arg(LIKE_COUNT_UPDATES_CHANNEL)
        .arg(payload)
        .query_async(&mut redis_connection)
        .await?;
    Ok(())
}

pub(crate) fn apply_like_count_update(state: &AppState, update: &LikeCountCacheUpdate) {
    let key = format!("likes:count:{}:{}", update.content_type, update.content_id);
    store_local_like_count(state, &key, update.count);
}

pub(super) async fn get_cached_json<T>(state: &AppState, key: &str) -> Result<Option<T>, AppError>
where
    T: DeserializeOwned,
{
    let mut redis_connection = state.get_redis_connection().await?;
    let payload: Option<String> = redis_connection.get(key).await?;

    match payload {
        Some(value) => match serde_json::from_str::<T>(&value) {
            Ok(decoded) => Ok(Some(decoded)),
            Err(error) => {
                tracing::warn!(
                    service = "likes_service",
                    error = %error,
                    cache_key = key,
                    "failed to decode cached value"
                );
                Ok(None)
            }
        },
        None => Ok(None),
    }
}

pub(super) async fn set_cached_json<T>(
    state: &AppState,
    key: &str,
    ttl_seconds: u64,
    value: &T,
) -> Result<(), AppError>
where
    T: Serialize,
{
    let payload = serde_json::to_string(value).map_err(|error| {
        AppError::dependency_unavailable(
            "CACHE_SERIALIZATION_ERROR",
            format!("failed to encode cached value: {error}"),
        )
    })?;
    let mut redis_connection = state.get_redis_connection().await?;
    let _: () = redis_connection.set_ex(key, payload, ttl_seconds).await?;
    Ok(())
}

pub(super) async fn get_cached_like_status(
    state: &AppState,
    user_id: &UserId,
    content_type: &ContentType,
    content_id: &ContentId,
) -> Result<Option<LikeStatusResponse>, AppError> {
    let key = like_status_cache_key(user_id, content_type, content_id);
    get_cached_json(state, &key).await
}

pub(super) async fn cache_like_status(
    state: &AppState,
    user_id: &UserId,
    content_type: &ContentType,
    content_id: &ContentId,
    status: &LikeStatusResponse,
) -> Result<(), AppError> {
    let key = like_status_cache_key(user_id, content_type, content_id);
    set_cached_json(state, &key, state.cache_ttl_user_status_seconds, status).await
}

pub(super) async fn get_cached_top_likes(
    state: &AppState,
    key: &str,
) -> Result<Option<TopLikesResponse>, AppError> {
    get_cached_json(state, key).await
}

pub(super) async fn cache_top_likes(
    state: &AppState,
    key: &str,
    response: &TopLikesResponse,
) -> Result<(), AppError> {
    set_cached_json(
        state,
        key,
        state.leaderboard_refresh_interval_seconds,
        response,
    )
    .await
}

pub(super) fn store_local_like_count(state: &AppState, key: &str, count: i64) {
    state.local_like_count_cache.set(
        key.to_string(),
        count,
        Duration::from_millis(LOCAL_LIKE_COUNT_CACHE_TTL_MS),
    );
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

#[cfg(test)]
mod tests {
    use super::*;
    use axum::response::IntoResponse;

    #[test]
    fn cursor_roundtrip_preserves_fields() {
        let row = UserLikeRow {
            content_type: "post".to_string(),
            content_id: "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaa1".to_string(),
            liked_at: "2026-02-02T17:00:00Z".to_string(),
        };

        let encoded = encode_cursor(&row);
        let decoded = decode_cursor(&encoded).expect("encoded cursor must decode");

        assert_eq!(decoded.liked_at, row.liked_at);
        assert_eq!(decoded.content_id, row.content_id);
    }

    #[test]
    fn decode_cursor_rejects_malformed_payload() {
        let error = decode_cursor("not-base64").expect_err("invalid cursor must fail");
        let response = error.into_response();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
}
