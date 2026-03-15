mod config;
mod domain;
mod error;
mod logging;

use axum::{middleware, routing::get, Json, Router};
use config::ServiceConfig;
use logging::{init_tracing, request_logging_middleware};
use serde_json::json;

#[tokio::main]
async fn main() {
    init_tracing();

    let config = ServiceConfig::from_env().unwrap_or_else(|error| {
        eprintln!("configuration error: {error}");
        std::process::exit(1);
    });

    let app = Router::new()
        .route("/health/live", get(live_health))
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
