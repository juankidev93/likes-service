mod app_state;
mod auth_middleware;
mod config;
mod domain;
mod error;
mod health;
mod http;
mod infra;
mod integrations;
mod mock_content_api;
mod mock_profile_api;
mod resilience;
mod storage;
mod use_cases;
#[cfg(test)]
mod integration_tests;

use config::ServiceConfig;
use infra::bootstrap::{build_app, build_app_state};
use infra::logging::init_tracing;
use infra::metrics::init_metrics;
use std::net::SocketAddr;
use std::time::Duration;

#[tokio::main]
async fn main() {
    init_tracing();
    init_metrics();

    let config = ServiceConfig::from_env().unwrap_or_else(|error| {
        eprintln!("configuration error: {error}");
        std::process::exit(1);
    });

    let app_state = build_app_state(&config).await;
    let shutdown_handle = app_state.shutdown_signal.clone();
    let app = build_app(app_state);

    let listener = tokio::net::TcpListener::bind(config.bind_address())
        .await
        .expect("failed to bind TCP listener");

    tracing::info!(service = "likes_service", address = %config.bind_address(), "starting HTTP server");

    let mut shutdown_receiver = shutdown_handle.subscribe();
    let server = axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(async move {
        while !*shutdown_receiver.borrow() {
            if shutdown_receiver.changed().await.is_err() {
                break;
            }
        }
    });

    let mut server_task = tokio::spawn(async move { server.await });
    let mut signal_task = tokio::spawn(shutdown_signal(shutdown_handle.clone()));

    tokio::select! {
        result = &mut server_task => {
            signal_task.abort();
            match result {
                Ok(Ok(())) => {}
                Ok(Err(error)) => panic!("HTTP server failed: {error}"),
                Err(error) => panic!("HTTP server task failed: {error}"),
            }
        }
        result = &mut signal_task => {
            if let Err(error) = result {
                panic!("shutdown signal task failed: {error}");
            }

            match tokio::time::timeout(Duration::from_secs(config.shutdown_timeout_secs), &mut server_task).await {
                Ok(Ok(Ok(()))) => {}
                Ok(Ok(Err(error))) => panic!("HTTP server failed: {error}"),
                Ok(Err(error)) => panic!("HTTP server task failed: {error}"),
                Err(_) => {
                    tracing::warn!(
                        service = "likes_service",
                        shutdown_timeout_secs = config.shutdown_timeout_secs,
                        "graceful shutdown timed out; aborting remaining tasks"
                    );
                }
            }
        }
    }

    tracing::info!(service = "likes_service", "HTTP server stopped");
}

async fn shutdown_signal(shutdown_handle: crate::infra::shutdown::ShutdownSignal) {
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
            tracing::info!(service = "likes_service", "received Ctrl+C, starting graceful shutdown");
        }
        _ = terminate => {
            tracing::info!(service = "likes_service", "received SIGTERM, starting graceful shutdown");
        }
    }

    shutdown_handle.trigger();
}
