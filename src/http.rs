use crate::app_state::AppState;
use crate::domain::{ContentId, ContentType, UserId};
use crate::error::AppError;
use crate::likes_repository::{LikesCursor, PostgresLikesRepository, UserLikeRow};
use crate::profile_api_client::AuthenticatedUser;
use crate::use_cases::{LikeContentResult, LikesUseCases, UnlikeContentResult};
use crate::metrics::record_cache_operation;
use axum::{
    extract::{Extension, Path, Query, State},
    http::StatusCode,
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
    );

    match use_cases
        .like_content(&user_id, &content_type, &content_id)
        .await
    {
        Ok(result) => {
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

pub async fn delete_like(
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
    );

    match use_cases
        .unlike_content(&user_id, &content_type, &content_id)
        .await
    {
        Ok(result) => success(Json(UnlikeResponse::from(result))).into_response(),
        Err(error) => error.into_response(),
    }
}

pub async fn get_like_status(
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

    match repository
        .get_like_status(&user_id, &content_type, &content_id)
        .await
    {
        Ok(status) => success(Json(LikeStatusResponse::from(status))).into_response(),
        Err(error) => error.into_response(),
    }
}

pub async fn get_like_count(
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

    let repository = PostgresLikesRepository::new(&state.db_pool);
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
            return error.into_response();
        }
    }

    match repository.get_like_count(&content_type, &content_id).await {
        Ok(count) => {
            if let Err(error) = cache_like_count(&state, &cache_key, count.count).await {
                return error.into_response();
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

pub async fn list_user_likes(
    State(state): State<AppState>,
    Extension(authenticated_user): Extension<AuthenticatedUser>,
    Query(query): Query<UserLikesQuery>,
) -> Response {
    let user_id = match parse_authenticated_user_id(&authenticated_user) {
        Ok(value) => value,
        Err(error) => return error.into_response(),
    };

    let limit = match parse_limit(query.limit) {
        Ok(value) => value,
        Err(error) => return error.into_response(),
    };

    let content_type = match query.content_type {
        Some(value) => match ContentType::from_str(&value) {
            Ok(content_type) => Some(content_type),
            Err(error) => return AppError::from(error).into_response(),
        },
        None => None,
    };

    let cursor = match query.cursor {
        Some(value) => match decode_cursor(&value) {
            Ok(cursor) => Some(cursor),
            Err(error) => return error.into_response(),
        },
        None => None,
    };

    let repository = PostgresLikesRepository::new(&state.db_pool);
    let rows = match repository
        .list_user_likes(&user_id, content_type.as_ref(), cursor.as_ref(), limit + 1)
        .await
    {
        Ok(rows) => rows,
        Err(error) => return error.into_response(),
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

    success(Json(UserLikesResponse {
        items,
        next_cursor,
        has_more: has_next_page,
    }))
    .into_response()
}

pub async fn get_like_counts_batch(
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
            return error.into_response();
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
                return error.into_response();
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

pub async fn get_like_statuses_batch(
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
    pub liked: bool,
    pub already_existed: bool,
    pub count: i64,
    pub liked_at: Option<String>,
}

impl From<LikeContentResult> for LikeResponse {
    fn from(value: LikeContentResult) -> Self {
        Self {
            liked: value.liked,
            already_existed: value.already_existed,
            count: value.count,
            liked_at: value.liked_at,
        }
    }
}

#[derive(Serialize)]
pub struct UnlikeResponse {
    pub liked: bool,
    pub was_liked: bool,
    pub count: i64,
}

impl From<UnlikeContentResult> for UnlikeResponse {
    fn from(value: UnlikeContentResult) -> Self {
        Self {
            liked: value.liked,
            was_liked: value.was_liked,
            count: value.count,
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
    pub content_type: String,
    pub content_id: String,
    pub count: i64,
}

impl LikeCountResponse {
    fn from_parts(content_type: &ContentType, content_id: &ContentId, count: i64) -> Self {
        Self {
            content_type: content_type.to_string(),
            content_id: content_id.to_string(),
            count,
        }
    }
}

#[derive(Serialize)]
pub struct BatchLikeCountsResponse {
    pub results: Vec<BatchLikeCountItemResponse>,
}

#[derive(Serialize)]
pub struct BatchLikeCountItemResponse {
    pub content_type: String,
    pub content_id: String,
    pub count: i64,
}

#[derive(Serialize)]
pub struct BatchLikeStatusesResponse {
    pub results: Vec<BatchLikeStatusItemResponse>,
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
    pub has_more: bool,
}

#[derive(Serialize)]
pub struct UserLikeItemResponse {
    pub content_type: String,
    pub content_id: String,
    pub liked_at: String,
}

fn parse_authenticated_user_id(
    authenticated_user: &AuthenticatedUser,
) -> Result<UserId, AppError> {
    UserId::from_str(&authenticated_user.user_id).map_err(AppError::from)
}

fn success<T>(payload: Json<T>) -> (StatusCode, Json<T>) {
    (StatusCode::OK, payload)
}

fn like_count_cache_key(content_type: &ContentType, content_id: &ContentId) -> String {
    format!("likes:count:{content_type}:{content_id}")
}

fn parse_limit(limit: Option<usize>) -> Result<usize, AppError> {
    let limit = limit.unwrap_or(DEFAULT_USER_LIKES_LIMIT);

    if limit == 0 || limit > MAX_BATCH_ITEMS {
        return Err(AppError::invalid_request(
            "INVALID_REQUEST",
            format!("limit must be between 1 and {MAX_BATCH_ITEMS}"),
        ));
    }

    Ok(limit)
}

fn encode_cursor(row: &UserLikeRow) -> String {
    let raw = format!("{}|{}", row.liked_at, row.content_id);
    STANDARD.encode(raw)
}

fn decode_cursor(value: &str) -> Result<LikesCursor, AppError> {
    let decoded = STANDARD
        .decode(value)
        .map_err(|_| AppError::invalid_request("INVALID_REQUEST", "invalid cursor"))?;

    let decoded =
        String::from_utf8(decoded).map_err(|_| AppError::invalid_request("INVALID_REQUEST", "invalid cursor"))?;

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
) -> Result<Vec<(ContentType, ContentId)>, AppError> {
    items
        .iter()
        .map(|item| {
            let content_type = ContentType::from_str(&item.content_type)
                .map_err(AppError::from)?;

            let content_id = ContentId::from_str(&item.content_id)
                .map_err(AppError::from)?;

            Ok((content_type, content_id))
        })
        .collect()
}
