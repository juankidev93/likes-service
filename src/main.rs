mod app_state;
mod auth_middleware;
mod bootstrap;
mod circuit_breaker;
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
#[cfg(test)]
mod integration_tests;

use bootstrap::{build_app, build_app_state};
use config::ServiceConfig;
use logging::init_tracing;
use metrics::init_metrics;
use std::net::SocketAddr;

#[tokio::main]
async fn main() {
    init_tracing();
    init_metrics();

    let config = ServiceConfig::from_env().unwrap_or_else(|error| {
        eprintln!("configuration error: {error}");
        std::process::exit(1);
    });

    let app_state = build_app_state(&config).await;
    let app = build_app(app_state);

    let listener = tokio::net::TcpListener::bind(config.bind_address())
        .await
        .expect("failed to bind TCP listener");

    tracing::info!(address = %config.bind_address(), "starting HTTP server");

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
        .with_graceful_shutdown(shutdown_signal())
        .await
        .expect("HTTP server failed");

    tracing::info!("HTTP server stopped");
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C signal handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {
            tracing::info!("received Ctrl+C, starting graceful shutdown");
        }
        _ = terminate => {
            tracing::info!("received SIGTERM, starting graceful shutdown");
        }
    }
}
