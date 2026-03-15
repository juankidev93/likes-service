mod app_state;
mod config;
mod domain;
mod error;
mod likes_repository;
mod logging;
mod use_cases;

use axum::{middleware, routing::get, Json, Router};
use app_state::AppState;
use config::ServiceConfig;
use logging::{init_tracing, request_logging_middleware};
use serde_json::json;
use sqlx::postgres::PgPoolOptions;
use std::time::Duration;

#[tokio::main]
async fn main() {
    init_tracing();

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

    let app_state = AppState { db_pool };

    let app = Router::new()
        .route("/health/live", get(live_health))
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
