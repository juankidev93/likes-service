use crate::app_state::{AppState, MockProfile};
use crate::config::ServiceConfig;
use crate::circuit_breaker::CircuitBreaker;
use crate::content_registry::{ContentApiDefinition, ContentTypeRegistry};
use crate::content_validation::ContentValidationClient;
use crate::health::ready_health;
use crate::http::{
    build_authenticated_read_routes, build_authenticated_write_routes, build_public_read_routes,
    live_health,
};
use crate::logging::request_logging_middleware;
use crate::metrics::metrics_handler;
use crate::mock_content_api::{build_mock_content_store, get_content};
use crate::mock_profile_api::validate_token;
use crate::profile_api_client::ProfileApiClient;
use crate::shutdown::ShutdownSignal;
use axum::{middleware, routing::get, Router};
use redis::AsyncCommands;
use sqlx::postgres::PgPoolOptions;
use std::collections::HashMap;
use std::time::Duration;

pub async fn build_app_state(config: &ServiceConfig) -> AppState {
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

    let _: String = redis_connection.ping().await.unwrap_or_else(|error| {
        eprintln!("redis ping error: {error}");
        std::process::exit(1);
    });

    let mock_profiles = build_mock_profiles();
    let mock_content_store = build_mock_content_store();
    let content_type_registry = build_content_type_registry(config);
    let profile_api_circuit_breaker = CircuitBreaker::new(
        "profile_api",
        config.circuit_breaker_failure_threshold,
        Duration::from_secs(config.circuit_breaker_open_seconds),
    );
    let content_api_circuit_breaker = CircuitBreaker::new(
        "content_api",
        config.circuit_breaker_failure_threshold,
        Duration::from_secs(config.circuit_breaker_open_seconds),
    );
    let profile_api_client = ProfileApiClient::new(
        config.profile_api_base_url.clone(),
        profile_api_circuit_breaker,
    );
    let content_validation_client = ContentValidationClient::new(
        content_type_registry.clone(),
        content_api_circuit_breaker,
    );
    let shutdown_signal = ShutdownSignal::new();

    AppState {
        db_pool,
        redis_client,
        write_rate_limit_per_minute: config.write_rate_limit_per_minute,
        read_rate_limit_per_minute: config.read_rate_limit_per_minute,
        mock_profiles,
        mock_content_store,
        content_type_registry,
        content_validation_client,
        profile_api_client,
        shutdown_signal,
    }
}

pub fn build_app(app_state: AppState) -> Router {
    let authenticated_write_routes = build_authenticated_write_routes(app_state.clone());
    let authenticated_read_routes = build_authenticated_read_routes(app_state.clone());
    let public_read_routes = build_public_read_routes(app_state.clone());

    Router::new()
        .route("/health/live", get(live_health))
        .route("/health/ready", get(ready_health))
        .route("/metrics", get(metrics_handler))
        .route("/v1/auth/validate", get(validate_token))
        .route("/v1/{content_type}/{content_id}", get(get_content))
        .merge(public_read_routes)
        .merge(authenticated_write_routes)
        .merge(authenticated_read_routes)
        .with_state(app_state)
        .layer(middleware::from_fn(request_logging_middleware))
}

fn build_mock_profiles() -> HashMap<String, MockProfile> {
    HashMap::from([
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
    ])
}

fn build_content_type_registry(config: &ServiceConfig) -> ContentTypeRegistry {
    ContentTypeRegistry::new(vec![
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
    ])
}
