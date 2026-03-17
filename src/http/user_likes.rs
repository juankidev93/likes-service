use crate::app_state::AppState;
use crate::domain::ContentType;
use crate::error::AppError;
use crate::likes_repository::PostgresLikesRepository;
use crate::profile_api_client::AuthenticatedUser;
use axum::{
    extract::{Extension, Query, State},
    response::{IntoResponse, Response},
    Json,
};
use std::str::FromStr;

use super::dto::{UserLikeItemResponse, UserLikesQuery, UserLikesResponse};
use super::helpers::{decode_cursor, encode_cursor, parse_authenticated_user_id, parse_limit, success};

pub(crate) async fn list_user_likes(
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
