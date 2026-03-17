use crate::app_state::AppState;
use crate::domain::ContentType;
use crate::error::AppError;
use crate::likes_repository::PostgresLikesRepository;
use axum::{
    extract::{Query, State},
    response::{IntoResponse, Response},
    Json,
};
use std::str::FromStr;

use super::dto::{TopLikeItemResponse, TopLikesQuery, TopLikesResponse};
use super::helpers::{parse_top_likes_limit, parse_top_likes_window, success};

pub(crate) async fn get_top_likes(
    State(state): State<AppState>,
    Query(query): Query<TopLikesQuery>,
) -> Response {
    let window = match parse_top_likes_window(query.window.as_deref()) {
        Ok(window) => window,
        Err(error) => return error.into_response(),
    };

    let limit = match parse_top_likes_limit(query.limit) {
        Ok(limit) => limit,
        Err(error) => return error.into_response(),
    };

    let content_type = match query.content_type {
        Some(value) => match ContentType::from_str(&value) {
            Ok(content_type) => Some(content_type),
            Err(error) => return AppError::from(error).into_response(),
        },
        None => None,
    };

    let repository = PostgresLikesRepository::new(&state.db_pool);
    let rows = match repository
        .list_top_likes(content_type.as_ref(), &window, limit)
        .await
    {
        Ok(rows) => rows,
        Err(error) => return error.into_response(),
    };

    let results = rows
        .into_iter()
        .map(|row| TopLikeItemResponse {
            content_type: row.content_type,
            content_id: row.content_id,
            count: row.like_count,
        })
        .collect();

    success(Json(TopLikesResponse {
        window: window.as_str().to_string(),
        results,
    }))
    .into_response()
}
