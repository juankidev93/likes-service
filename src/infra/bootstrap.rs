use crate::app_state::{AppState, MockProfile};
use crate::config::ServiceConfig;
use crate::health::ready_health;
use crate::http::{
    build_authenticated_read_routes, build_authenticated_write_routes, build_public_read_routes,
    live_health, openapi_spec, swagger_ui,
};
use crate::infra::logging::request_logging_middleware;
use crate::infra::metrics::metrics_handler;
use crate::infra::shutdown::ShutdownSignal;
use crate::integrations::content_registry::{ContentApiDefinition, ContentTypeRegistry};
use crate::integrations::content_validation::ContentValidationClient;
use crate::integrations::profile_api_client::ProfileApiClient;
use crate::integrations::sse_events::LikeEvents;
use crate::mock_content_api::{build_mock_content_store, get_content};
use crate::mock_profile_api::validate_token;
use crate::resilience::circuit_breaker::CircuitBreaker;
use axum::{Router, middleware, routing::get};
use redis::AsyncCommands;
use sqlx::postgres::PgPoolOptions;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

pub async fn build_app_state(config: &ServiceConfig) -> AppState {
    let db_pool = PgPoolOptions::new()
        .max_connections(config.db_max_connections)
        .min_connections(config.db_min_connections)
        .acquire_timeout(Duration::from_secs(config.db_acquire_timeout_secs))
        .connect(&config.database_url)
        .await
        .unwrap_or_else(|error| {
            eprintln!("database connection error: {error}");
            std::process::exit(1);
        });

    let read_db_pool = PgPoolOptions::new()
        .max_connections(config.db_max_connections)
        .min_connections(config.db_min_connections)
        .acquire_timeout(Duration::from_secs(config.db_acquire_timeout_secs))
        .connect(&config.read_database_url)
        .await
        .unwrap_or_else(|error| {
            eprintln!("read database connection error: {error}");
            std::process::exit(1);
        });

    validate_required_schema(&db_pool).await;
    validate_required_schema(&read_db_pool).await;

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
        config.circuit_breaker_success_threshold,
        Duration::from_secs(config.circuit_breaker_failure_window_seconds),
    );
    let content_api_circuit_breaker = CircuitBreaker::new(
        "content_api",
        config.circuit_breaker_failure_threshold,
        Duration::from_secs(config.circuit_breaker_open_seconds),
        config.circuit_breaker_success_threshold,
        Duration::from_secs(config.circuit_breaker_failure_window_seconds),
    );
    let profile_api_client = ProfileApiClient::new(
        config.profile_api_base_url.clone(),
        profile_api_circuit_breaker,
    );
    let content_validation_client = ContentValidationClient::new(
        content_type_registry.clone(),
        redis_client.clone(),
        config.cache_ttl_content_validation_seconds,
        content_api_circuit_breaker,
    );
    let shutdown_signal = ShutdownSignal::new();
    let like_events = LikeEvents::new(redis_client.clone());

    AppState {
        db_pool,
        read_db_pool,
        redis_client,
        redis_connection: Some(redis_connection.clone()),
        cache_ttl_like_counts_seconds: config.cache_ttl_like_counts_seconds,
        cache_ttl_user_status_seconds: config.cache_ttl_user_status_seconds,
        leaderboard_refresh_interval_seconds: config.leaderboard_refresh_interval_seconds,
        write_rate_limit_per_minute: config.write_rate_limit_per_minute,
        read_rate_limit_per_minute: config.read_rate_limit_per_minute,
        sse_heartbeat_interval_seconds: config.sse_heartbeat_interval_seconds,
        local_like_count_cache: Arc::new(Default::default()),
        like_count_cache_inflight: Arc::new(Default::default()),
        mock_profiles,
        mock_content_store,
        content_type_registry,
        content_validation_client,
        profile_api_client,
        shutdown_signal,
        like_events,
    }
}

pub fn build_app(app_state: AppState) -> Router {
    let authenticated_write_routes = build_authenticated_write_routes(app_state.clone());
    let authenticated_read_routes = build_authenticated_read_routes(app_state.clone());
    let public_read_routes = build_public_read_routes(app_state.clone());

    Router::new()
        .route("/docs", get(swagger_ui))
        .route("/openapi.yaml", get(openapi_spec))
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
            "tok_user_1".to_string(),
            MockProfile {
                user_id: "11111111-1111-1111-1111-111111111111".to_string(),
                display_name: "Test User 1".to_string(),
            },
        ),
        (
            "tok_user_2".to_string(),
            MockProfile {
                user_id: "22222222-2222-2222-2222-222222222222".to_string(),
                display_name: "Test User 2".to_string(),
            },
        ),
        (
            "tok_user_3".to_string(),
            MockProfile {
                user_id: "33333333-3333-3333-3333-333333333333".to_string(),
                display_name: "Test User 3".to_string(),
            },
        ),
        (
            "tok_user_4".to_string(),
            MockProfile {
                user_id: "44444444-4444-4444-4444-444444444444".to_string(),
                display_name: "Test User 4".to_string(),
            },
        ),
        (
            "tok_user_5".to_string(),
            MockProfile {
                user_id: "55555555-5555-5555-5555-555555555555".to_string(),
                display_name: "Test User 5".to_string(),
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

async fn validate_required_schema(db_pool: &sqlx::PgPool) {
    let missing_table_count = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*)
        FROM (
            VALUES ('likes'), ('like_counts'), ('like_hourly_counts')
        ) AS required_tables(table_name)
        WHERE NOT EXISTS (
            SELECT 1
            FROM information_schema.tables
            WHERE table_schema = 'public'
              AND table_name = required_tables.table_name
        )
        "#,
    )
    .fetch_one(db_pool)
    .await
    .unwrap_or_else(|error| {
        eprintln!("database schema validation error: {error}");
        std::process::exit(1);
    });

    if missing_table_count > 0 {
        eprintln!(
            "database schema is missing required tables; apply versioned migrations before starting the service"
        );
        std::process::exit(1);
    }
}
