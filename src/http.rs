use crate::app_state::AppState;
use crate::domain::{ContentId, ContentType, UserId};
use crate::error::AppError;
use crate::likes_repository::{LikesCursor, PostgresLikesRepository, UserLikeRow};
use crate::use_cases::{LikeContentResult, LikesUseCases, UnlikeContentResult};
use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::str::FromStr;

const DEFAULT_USER_LIKES_LIMIT: usize = 20;
const MAX_BATCH_ITEMS: usize = 100;
const LIKE_COUNT_CACHE_TTL_SECONDS: u64 = 60;

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
    let use_cases = LikesUseCases::new(repository, state.redis_client.clone());

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
    let use_cases = LikesUseCases::new(repository, state.redis_client.clone());

    match use_cases
        .unlike_content(&user_id, &content_type, &content_id)
        .await
    {
        Ok(result) => success(Json(UnlikeResponse::from(result))).into_response(),
        Err(error) => internal_error(error).into_response(),
    }
}

pub async fn get_like_status(
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

    match repository
        .get_like_status(&user_id, &content_type, &content_id)
        .await
    {
        Ok(status) => success(Json(LikeStatusResponse::from(status))).into_response(),
        Err(error) => internal_error(error).into_response(),
    }
}

pub async fn get_like_count(
    State(state): State<AppState>,
    Path((content_type, content_id)): Path<(String, String)>,
) -> Response {
    let content_type = match ContentType::from_str(&content_type) {
        Ok(value) => value,
        Err(error) => return bad_request(error.to_string()).into_response(),
    };

    let content_id = match ContentId::from_str(&content_id) {
        Ok(value) => value,
        Err(error) => return bad_request(error.to_string()).into_response(),
    };

    let repository = PostgresLikesRepository::new(&state.db_pool);
    let cache_key = like_count_cache_key(&content_type, &content_id);

    match get_cached_like_count(&state, &cache_key).await {
        Ok(Some(count)) => {
            return success(Json(LikeCountResponse { count })).into_response();
        }
        Ok(None) => {}
        Err(error) => return internal_error(error).into_response(),
    }

    match repository.get_like_count(&content_type, &content_id).await {
        Ok(count) => {
            if let Err(error) = cache_like_count(&state, &cache_key, count.count).await {
                return internal_error(error).into_response();
            }

            success(Json(LikeCountResponse::from(count))).into_response()
        }
        Err(error) => internal_error(error).into_response(),
    }
}

pub async fn list_user_likes(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<UserLikesQuery>,
) -> Response {
    let user_id = match parse_user_id(&headers) {
        Ok(value) => value,
        Err(response) => return response.into_response(),
    };

    let limit = match parse_limit(query.limit) {
        Ok(value) => value,
        Err(response) => return response.into_response(),
    };

    let content_type = match query.content_type {
        Some(value) => match ContentType::from_str(&value) {
            Ok(content_type) => Some(content_type),
            Err(error) => return bad_request(error.to_string()).into_response(),
        },
        None => None,
    };

    let cursor = match query.cursor {
        Some(value) => match decode_cursor(&value) {
            Ok(cursor) => Some(cursor),
            Err(response) => return response.into_response(),
        },
        None => None,
    };

    let repository = PostgresLikesRepository::new(&state.db_pool);
    let rows = match repository
        .list_user_likes(&user_id, content_type.as_ref(), cursor.as_ref(), limit + 1)
        .await
    {
        Ok(rows) => rows,
        Err(error) => return internal_error(error).into_response(),
    };

    let has_next_page = rows.len() > limit;
    let page_rows = if has_next_page {
        rows[..limit].to_vec()
    } else {
        rows
    };

    let next_cursor = if has_next_page {
        page_rows.last().map(encode_cursor)
    } else {
        None
    };

    let items = page_rows
        .into_iter()
        .map(|row| UserLikeItemResponse {
            content_type: row.content_type,
            content_id: row.content_id,
            liked_at: row.liked_at,
        })
        .collect();

    success(Json(UserLikesResponse { items, next_cursor })).into_response()
}

pub async fn get_like_counts_batch(
    State(state): State<AppState>,
    Json(payload): Json<BatchLikesRequest>,
) -> Response {
    if payload.items.len() > MAX_BATCH_ITEMS {
        return bad_request("BATCH_TOO_LARGE".to_string()).into_response();
    }

    let parsed_items = match parse_batch_items(&payload.items) {
        Ok(items) => items,
        Err(response) => return response.into_response(),
    };

    let cache_keys: Vec<String> = parsed_items
        .iter()
        .map(|(content_type, content_id)| like_count_cache_key(content_type, content_id))
        .collect();

    let cached_counts = match get_cached_like_counts(&state, &cache_keys).await {
        Ok(values) => values,
        Err(error) => return internal_error(error).into_response(),
    };

    let mut counts_by_item: HashMap<(String, String), i64> = HashMap::new();
    let mut missing_items = Vec::new();

    for ((content_type, content_id), cached_count) in parsed_items.iter().zip(cached_counts) {
        let key = (content_type.to_string(), content_id.to_string());

        if let Some(count) = cached_count {
            counts_by_item.insert(key, count);
        } else {
            missing_items.push((content_type.clone(), content_id.clone()));
        }
    }

    if !missing_items.is_empty() {
        let repository = PostgresLikesRepository::new(&state.db_pool);
        let postgres_counts = match repository.get_like_counts_batch(&missing_items).await {
            Ok(values) => values,
            Err(error) => return internal_error(error).into_response(),
        };

        for (content_type, content_id) in &missing_items {
            let key = (content_type.to_string(), content_id.to_string());
            let count = *postgres_counts.get(&key).unwrap_or(&0);

            counts_by_item.insert(key.clone(), count);

            if let Err(error) =
                cache_like_count(&state, &like_count_cache_key(content_type, content_id), count)
                    .await
            {
                return internal_error(error).into_response();
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

    success(Json(BatchLikeCountsResponse { items })).into_response()
}

pub async fn get_like_statuses_batch(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<BatchLikesRequest>,
) -> Response {
    if payload.items.len() > MAX_BATCH_ITEMS {
        return bad_request("BATCH_TOO_LARGE".to_string()).into_response();
    }

    let user_id = match parse_user_id(&headers) {
        Ok(value) => value,
        Err(response) => return response.into_response(),
    };

    let parsed_items = match parse_batch_items(&payload.items) {
        Ok(items) => items,
        Err(response) => return response.into_response(),
    };

    let repository = PostgresLikesRepository::new(&state.db_pool);
    let statuses = match repository
        .get_like_statuses_batch(&user_id, &parsed_items)
        .await
    {
        Ok(values) => values,
        Err(error) => return internal_error(error).into_response(),
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

    success(Json(BatchLikeStatusesResponse { items })).into_response()
}

#[derive(Deserialize)]
pub struct CreateLikeRequest {
    pub content_type: String,
    pub content_id: String,
}

#[derive(Deserialize)]
pub struct BatchLikesRequest {
    pub items: Vec<BatchLikeItemRequest>,
}

#[derive(Deserialize)]
pub struct BatchLikeItemRequest {
    pub content_type: String,
    pub content_id: String,
}

#[derive(Deserialize)]
pub struct UserLikesQuery {
    pub limit: Option<usize>,
    pub cursor: Option<String>,
    pub content_type: Option<String>,
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
pub struct LikeStatusResponse {
    pub liked: bool,
    pub liked_at: Option<String>,
}

impl From<crate::likes_repository::LikeStatus> for LikeStatusResponse {
    fn from(value: crate::likes_repository::LikeStatus) -> Self {
        Self {
            liked: value.exists,
            liked_at: value.liked_at,
        }
    }
}

#[derive(Serialize)]
pub struct LikeCountResponse {
    pub count: i64,
}

impl From<crate::likes_repository::LikeCount> for LikeCountResponse {
    fn from(value: crate::likes_repository::LikeCount) -> Self {
        Self { count: value.count }
    }
}

#[derive(Serialize)]
pub struct BatchLikeCountsResponse {
    pub items: Vec<BatchLikeCountItemResponse>,
}

#[derive(Serialize)]
pub struct BatchLikeCountItemResponse {
    pub content_type: String,
    pub content_id: String,
    pub count: i64,
}

#[derive(Serialize)]
pub struct BatchLikeStatusesResponse {
    pub items: Vec<BatchLikeStatusItemResponse>,
}

#[derive(Serialize)]
pub struct BatchLikeStatusItemResponse {
    pub content_type: String,
    pub content_id: String,
    pub liked: bool,
    pub liked_at: Option<String>,
}

#[derive(Serialize)]
pub struct UserLikesResponse {
    pub items: Vec<UserLikeItemResponse>,
    pub next_cursor: Option<String>,
}

#[derive(Serialize)]
pub struct UserLikeItemResponse {
    pub content_type: String,
    pub content_id: String,
    pub liked_at: String,
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

fn like_count_cache_key(content_type: &ContentType, content_id: &ContentId) -> String {
    format!("likes:count:{content_type}:{content_id}")
}

fn parse_limit(limit: Option<usize>) -> Result<usize, (StatusCode, Json<ErrorResponse>)> {
    let limit = limit.unwrap_or(DEFAULT_USER_LIKES_LIMIT);

    if limit == 0 || limit > MAX_BATCH_ITEMS {
        return Err(bad_request(format!(
            "limit must be between 1 and {MAX_BATCH_ITEMS}"
        )));
    }

    Ok(limit)
}

fn encode_cursor(row: &UserLikeRow) -> String {
    let raw = format!("{}|{}", row.liked_at, row.content_id);
    STANDARD.encode(raw)
}

fn decode_cursor(value: &str) -> Result<LikesCursor, (StatusCode, Json<ErrorResponse>)> {
    let decoded = STANDARD
        .decode(value)
        .map_err(|_| bad_request("invalid cursor".to_string()))?;

    let decoded =
        String::from_utf8(decoded).map_err(|_| bad_request("invalid cursor".to_string()))?;

    let (liked_at, content_id) = decoded
        .split_once('|')
        .ok_or_else(|| bad_request("invalid cursor".to_string()))?;

    if liked_at.is_empty() || content_id.is_empty() {
        return Err(bad_request("invalid cursor".to_string()));
    }

    Ok(LikesCursor {
        liked_at: liked_at.to_string(),
        content_id: content_id.to_string(),
    })
}

async fn get_cached_like_count(
    state: &AppState,
    key: &str,
) -> Result<Option<i64>, AppError> {
    let mut redis_connection = state.redis_client.get_multiplexed_async_connection().await?;
    let count = redis_connection.get(key).await?;
    Ok(count)
}

async fn get_cached_like_counts(
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

async fn cache_like_count(state: &AppState, key: &str, count: i64) -> Result<(), AppError> {
    let mut redis_connection = state.redis_client.get_multiplexed_async_connection().await?;
    let _: () = redis_connection
        .set_ex(key, count, LIKE_COUNT_CACHE_TTL_SECONDS)
        .await?;
    Ok(())
}

fn parse_batch_items(
    items: &[BatchLikeItemRequest],
) -> Result<Vec<(ContentType, ContentId)>, (StatusCode, Json<ErrorResponse>)> {
    items
        .iter()
        .map(|item| {
            let content_type = ContentType::from_str(&item.content_type)
                .map_err(|error| bad_request(error.to_string()))?;

            let content_id = ContentId::from_str(&item.content_id)
                .map_err(|error| bad_request(error.to_string()))?;

            Ok((content_type, content_id))
        })
        .collect()
}
