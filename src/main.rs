mod app_state;
mod auth_middleware;
mod config;
mod domain;
mod error;
mod grpc;
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
use grpc::{FILE_DESCRIPTOR_SET, GrpcLikesService};
use infra::bootstrap::{build_app, build_app_state, build_mock_content_app, build_mock_profile_app};
use infra::logging::init_tracing;
use infra::metrics::init_metrics;
use std::net::SocketAddr;
use std::time::Duration;
use tonic::transport::Server as GrpcServer;

#[tokio::main]
async fn main() {
    init_tracing();
    init_metrics();

    let run_mode = std::env::var("APP_MODE").unwrap_or_else(|_| "social-api".to_string());

    if run_mode == "mock-profile-api" || run_mode == "mock-content-api" {
        run_mock_service(&run_mode).await;
        return;
    }

    let config = ServiceConfig::from_env().unwrap_or_else(|error| {
        eprintln!("configuration error: {error}");
        std::process::exit(1);
    });

    let app_state = build_app_state(&config).await;
    let grpc_state = app_state.clone();
    let shutdown_handle = app_state.shutdown_signal.clone();
    let app = build_app(app_state);
    let grpc_port = std::env::var("GRPC_PORT").ok().and_then(|value| value.parse::<u16>().ok());

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
    let mut grpc_task = grpc_port.map(|port| {
        let grpc_address = format!("{}:{port}", config.host)
            .parse::<SocketAddr>()
            .unwrap_or_else(|error| {
                panic!("failed to parse gRPC bind address from SERVICE_HOST and GRPC_PORT: {error}")
            });

        tokio::spawn(serve_grpc(
            grpc_state,
            shutdown_handle.clone(),
            grpc_address,
        ))
    });

    tokio::select! {
        result = &mut server_task => {
            signal_task.abort();
            if let Some(grpc_task) = &grpc_task {
                grpc_task.abort();
            }
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

            if let Some(grpc_task) = &mut grpc_task {
                match tokio::time::timeout(Duration::from_secs(config.shutdown_timeout_secs), grpc_task).await {
                    Ok(Ok(Ok(()))) => {}
                    Ok(Ok(Err(error))) => panic!("gRPC server failed: {error}"),
                    Ok(Err(error)) => panic!("gRPC server task failed: {error}"),
                    Err(_) => {
                        tracing::warn!(
                            service = "likes_service",
                            shutdown_timeout_secs = config.shutdown_timeout_secs,
                            "gRPC shutdown timed out; aborting remaining tasks"
                        );
                    }
                }
            }
        }
    }

    tracing::info!(service = "likes_service", "HTTP server stopped");
}

async fn run_mock_service(run_mode: &str) {
    let host = std::env::var("SERVICE_HOST").unwrap_or_else(|_| "0.0.0.0".to_string());
    let port = std::env::var("HTTP_PORT")
        .unwrap_or_else(|_| "3000".to_string())
        .parse::<u16>()
        .unwrap_or_else(|error| {
            eprintln!("configuration error: HTTP_PORT must be a valid value, got parse error: {error}");
            std::process::exit(1);
        });
    let bind_address = format!("{host}:{port}");
    let shutdown_handle = crate::infra::shutdown::ShutdownSignal::new();

    let app = match run_mode {
        "mock-profile-api" => build_mock_profile_app(),
        "mock-content-api" => build_mock_content_app(),
        _ => unreachable!("validated above"),
    };

    let listener = tokio::net::TcpListener::bind(&bind_address)
        .await
        .expect("failed to bind TCP listener");

    tracing::info!(service = "likes_service", mode = run_mode, address = %bind_address, "starting mock service");

    axum::serve(listener, app.into_make_service())
        .with_graceful_shutdown(shutdown_signal(shutdown_handle))
        .await
        .expect("HTTP server failed");
}

async fn serve_grpc(
    app_state: crate::app_state::AppState,
    shutdown_handle: crate::infra::shutdown::ShutdownSignal,
    address: SocketAddr,
) -> Result<(), tonic::transport::Error> {
    let mut shutdown_receiver = shutdown_handle.subscribe();
    let grpc_service = GrpcLikesService::new(app_state);
    let reflection_service = tonic_reflection::server::Builder::configure()
        .register_encoded_file_descriptor_set(FILE_DESCRIPTOR_SET)
        .build_v1()
        .expect("failed to build gRPC reflection service");

    tracing::info!(service = "likes_service", grpc_address = %address, "starting gRPC server");

    GrpcServer::builder()
        .add_service(grpc_service.into_server())
        .add_service(reflection_service)
        .serve_with_shutdown(address, async move {
            while !*shutdown_receiver.borrow() {
                if shutdown_receiver.changed().await.is_err() {
                    break;
                }
            }
        })
        .await
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
