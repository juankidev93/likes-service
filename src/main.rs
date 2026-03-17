mod app_state;
mod auth_middleware;
mod config;
mod content_validation;
mod content_registry;
mod domain;
mod error;
mod health;
mod http;
mod likes_repository;
mod logging;
mod metrics;
mod mock_content_api;
mod mock_profile_api;
mod profile_api_client;
mod rate_limit;
mod use_cases;

use axum::{middleware, routing::{delete, get, post}, Json, Router};
use app_state::{AppState, MockProfile};
use auth_middleware::require_auth;
use config::ServiceConfig;
use content_validation::ContentValidationClient;
use content_registry::{ContentApiDefinition, ContentTypeRegistry};
use health::ready_health;
use http::{
    create_like, delete_like, get_like_count, get_like_counts_batch, get_like_status,
    get_like_statuses_batch, list_user_likes,
};
use logging::{init_tracing, request_logging_middleware};
use metrics::{init_metrics, metrics_handler};
use mock_content_api::{build_mock_content_store, get_content};
use mock_profile_api::validate_token;
use profile_api_client::ProfileApiClient;
use rate_limit::require_write_auth_and_rate_limit;
use redis::AsyncCommands;
use serde_json::json;
use sqlx::postgres::PgPoolOptions;
use std::collections::HashMap;
use std::time::Duration;

#[tokio::main]
async fn main() {
    init_tracing();
    init_metrics();

    let config = ServiceConfig::from_env().unwrap_or_else(|error| {
        eprintln!("configuration error: {error}");
        std::process::exit(1);
    });

    let db_pool = PgPoolOptions::new()
        .max_connections(5)
        .acquire_timeout(Duration::from_secs(5))
        .connect(&config.database_url)
        .await
        .unwrap_or_else(|error| {
            eprintln!("database connection error: {error}");
            std::process::exit(1);
        });

    let redis_client = redis::Client::open(config.redis_url.clone()).unwrap_or_else(|error| {
        eprintln!("redis configuration error: {error}");
        std::process::exit(1);
    });

    let mut redis_connection = redis_client
        .get_multiplexed_async_connection()
        .await
        .unwrap_or_else(|error| {
            eprintln!("redis connection error: {error}");
            std::process::exit(1);
        });

    let _: String = redis_connection
        .ping()
        .await
        .unwrap_or_else(|error| {
            eprintln!("redis ping error: {error}");
            std::process::exit(1);
        });

    let mock_profiles = HashMap::from([
        (
            "valid-alice-token".to_string(),
            MockProfile {
                user_id: "11111111-1111-1111-1111-111111111111".to_string(),
                display_name: "Alice".to_string(),
            },
        ),
        (
            "valid-bob-token".to_string(),
            MockProfile {
                user_id: "22222222-2222-2222-2222-222222222222".to_string(),
                display_name: "Bob".to_string(),
            },
        ),
        (
            "valid-charlie-token".to_string(),
            MockProfile {
                user_id: "33333333-3333-3333-3333-333333333333".to_string(),
                display_name: "Charlie".to_string(),
            },
        ),
    ]);

    let profile_api_client = ProfileApiClient::new(config.profile_api_base_url.clone());
    let mock_content_store = build_mock_content_store();
    let content_type_registry = ContentTypeRegistry::new(vec![
        ContentApiDefinition {
            content_type: "post".to_string(),
            base_url: config.post_content_api_base_url.clone(),
        },
        ContentApiDefinition {
            content_type: "bonus_hunter".to_string(),
            base_url: config.bonus_hunter_content_api_base_url.clone(),
        },
        ContentApiDefinition {
            content_type: "top_picks".to_string(),
            base_url: config.top_picks_content_api_base_url.clone(),
        },
    ]);
    let content_validation_client = ContentValidationClient::new(content_type_registry.clone());

    let app_state = AppState {
        db_pool,
        redis_client,
        write_rate_limit_per_minute: config.write_rate_limit_per_minute,
        mock_profiles,
        mock_content_store,
        content_type_registry,
        content_validation_client,
        profile_api_client,
    };

    let authenticated_write_routes = Router::new()
        .route("/v1/likes", post(create_like))
        .route("/v1/likes/{content_type}/{content_id}", delete(delete_like))
        .route_layer(middleware::from_fn_with_state(
            app_state.clone(),
            require_write_auth_and_rate_limit,
        ));

    let authenticated_read_routes = Router::new()
        .route("/v1/likes/user", get(list_user_likes))
        .route("/v1/likes/batch/statuses", post(get_like_statuses_batch))
        .route(
            "/v1/likes/{content_type}/{content_id}/status",
            get(get_like_status),
        )
        .route_layer(middleware::from_fn_with_state(
            app_state.clone(),
            require_auth,
        ));

    let app = Router::new()
        .route("/health/live", get(live_health))
        .route("/health/ready", get(ready_health))
        .route("/metrics", get(metrics_handler))
        .route("/v1/auth/validate", get(validate_token))
        .route("/v1/{content_type}/{content_id}", get(get_content))
        .route("/v1/likes/batch/counts", post(get_like_counts_batch))
        .route(
            "/v1/likes/{content_type}/{content_id}/count",
            get(get_like_count),
        )
        .merge(authenticated_write_routes)
        .merge(authenticated_read_routes)
        .with_state(app_state)
        .layer(middleware::from_fn(request_logging_middleware));

    let listener = tokio::net::TcpListener::bind(config.bind_address())
        .await
        .expect("failed to bind TCP listener");

    axum::serve(listener, app)
        .await
        .expect("HTTP server failed");
}

async fn live_health() -> Json<serde_json::Value> {
    Json(json!({ "status": "ok" }))
}
