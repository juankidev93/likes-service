use crate::app_state::AppState;
use crate::auth_middleware::require_auth;
use crate::rate_limit::{require_read_rate_limit, require_write_auth_and_rate_limit};
use axum::{
    middleware,
    routing::{delete, get, post},
    Json, Router,
};
use serde_json::json;

use super::batch::{get_like_counts_batch, get_like_statuses_batch};
use super::likes::{create_like, delete_like, get_like_count, get_like_status};
use super::stream::stream_like_events;
use super::top::get_top_likes;
use super::user_likes::list_user_likes;

pub fn build_authenticated_write_routes(state: AppState) -> Router<AppState> {
    Router::new()
        .route("/v1/likes", post(create_like))
        .route("/v1/likes/{content_type}/{content_id}", delete(delete_like))
        .route_layer(middleware::from_fn_with_state(
            state,
            require_write_auth_and_rate_limit,
        ))
}

pub fn build_authenticated_read_routes(state: AppState) -> Router<AppState> {
    Router::new()
        .route("/v1/likes/user", get(list_user_likes))
        .route("/v1/likes/batch/statuses", post(get_like_statuses_batch))
        .route(
            "/v1/likes/{content_type}/{content_id}/status",
            get(get_like_status),
        )
        .route_layer(middleware::from_fn_with_state(state, require_auth))
}

pub fn build_public_read_routes(state: AppState) -> Router<AppState> {
    Router::new()
        .route("/v1/likes/stream", get(stream_like_events))
        .route("/v1/likes/top", get(get_top_likes))
        .route("/v1/likes/batch/counts", post(get_like_counts_batch))
        .route(
            "/v1/likes/{content_type}/{content_id}/count",
            get(get_like_count),
        )
        .route_layer(middleware::from_fn_with_state(state, require_read_rate_limit))
}

pub async fn live_health() -> Json<serde_json::Value> {
    Json(json!({ "status": "ok" }))
}
