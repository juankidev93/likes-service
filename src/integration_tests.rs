use crate::bootstrap::{build_app, build_app_state};
use crate::config::ServiceConfig;
use crate::logging::init_tracing;
use crate::metrics::init_metrics;
use axum::{extract::Path, routing::get, Json, Router};
use reqwest::StatusCode;
use serde_json::{json, Value};
use serial_test::serial;
use sqlx::postgres::PgPoolOptions;
use std::env;
use std::net::SocketAddr;
use std::sync::Once;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tokio::time::{sleep, Duration};

static TEST_INIT: Once = Once::new();

#[tokio::test]
#[serial]
async fn create_like_without_token_returns_401() {
    let server = TestServer::spawn(|_| {}).await;

    let response = server
        .client
        .post(format!("{}/v1/likes", server.base_url))
        .json(&json!({
            "content_type": "post",
            "content_id": "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaa1"
        }))
        .send()
        .await
        .expect("request must succeed");

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let body: Value = response.json().await.expect("response must be valid json");
    assert_eq!(body["error"]["code"], "UNAUTHORIZED");

    server.shutdown().await;
}

#[tokio::test]
#[serial]
async fn create_like_with_unknown_content_returns_404() {
    let server = TestServer::spawn(|_| {}).await;

    let response = server
        .client
        .post(format!("{}/v1/likes", server.base_url))
        .bearer_auth("valid-alice-token")
        .json(&json!({
            "content_type": "post",
            "content_id": "ffffffff-ffff-ffff-ffff-ffffffffffff"
        }))
        .send()
        .await
        .expect("request must succeed");

    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    let body: Value = response.json().await.expect("response must be valid json");
    assert_eq!(body["error"]["code"], "CONTENT_NOT_FOUND");

    server.shutdown().await;
}

#[tokio::test]
#[serial]
async fn like_then_count_returns_updated_value() {
    let server = TestServer::spawn(|_| {}).await;

    let create_response = server
        .client
        .post(format!("{}/v1/likes", server.base_url))
        .bearer_auth("valid-alice-token")
        .json(&json!({
            "content_type": "post",
            "content_id": "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaa1"
        }))
        .send()
        .await
        .expect("create request must succeed");

    assert_eq!(create_response.status(), StatusCode::CREATED);

    let count_response = server
        .client
        .get(format!(
            "{}/v1/likes/post/aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaa1/count",
            server.base_url
        ))
        .send()
        .await
        .expect("count request must succeed");

    assert_eq!(count_response.status(), StatusCode::OK);

    let body: Value = count_response.json().await.expect("response must be valid json");
    assert_eq!(body["count"], 1);

    server.shutdown().await;
}

#[tokio::test]
#[serial]
async fn write_rate_limit_returns_429_when_exceeded() {
    let server = TestServer::spawn(|config| {
        config.write_rate_limit_per_minute = 1;
    })
    .await;

    let metrics_before = fetch_metrics(&server).await;
    let allowed_before = metric_value_or_zero(
        &metrics_before,
        "social_api_rate_limit_allowed_total",
        &[("scope", "write_user")],
    );
    let rejected_before = metric_value_or_zero(
        &metrics_before,
        "social_api_rate_limit_rejected_total",
        &[("scope", "write_user")],
    );

    let first_response = server
        .client
        .post(format!("{}/v1/likes", server.base_url))
        .bearer_auth("valid-alice-token")
        .json(&json!({
            "content_type": "post",
            "content_id": "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaa1"
        }))
        .send()
        .await
        .expect("first request must succeed");

    assert_eq!(first_response.status(), StatusCode::CREATED);

    let second_response = server
        .client
        .post(format!("{}/v1/likes", server.base_url))
        .bearer_auth("valid-alice-token")
        .json(&json!({
            "content_type": "post",
            "content_id": "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaa1"
        }))
        .send()
        .await
        .expect("second request must succeed");

    assert_eq!(second_response.status(), StatusCode::TOO_MANY_REQUESTS);

    let body: Value = second_response.json().await.expect("response must be valid json");
    assert_eq!(body["error"]["code"], "RATE_LIMITED");

    let metrics_after = fetch_metrics(&server).await;
    assert!(
        metric_value_or_zero(
            &metrics_after,
            "social_api_rate_limit_allowed_total",
            &[("scope", "write_user")],
        ) >= allowed_before + 1.0
    );
    assert!(
        metric_value_or_zero(
            &metrics_after,
            "social_api_rate_limit_rejected_total",
            &[("scope", "write_user")],
        ) >= rejected_before + 1.0
    );

    server.shutdown().await;
}

#[tokio::test]
#[serial]
async fn profile_api_circuit_breaker_opens_and_rejects_following_requests() {
    let server = TestServer::spawn(|config| {
        config.profile_api_base_url = "http://127.0.0.1:9".to_string();
        config.circuit_breaker_failure_threshold = 1;
        config.circuit_breaker_open_seconds = 60;
    })
    .await;

    let first_response = server
        .client
        .get(format!("{}/v1/likes/user", server.base_url))
        .bearer_auth("valid-alice-token")
        .send()
        .await
        .expect("first request must succeed");

    assert_eq!(first_response.status(), StatusCode::SERVICE_UNAVAILABLE);

    let second_response = server
        .client
        .get(format!("{}/v1/likes/user", server.base_url))
        .bearer_auth("valid-alice-token")
        .send()
        .await
        .expect("second request must succeed");

    assert_eq!(second_response.status(), StatusCode::SERVICE_UNAVAILABLE);

    let metrics_response = server
        .client
        .get(format!("{}/metrics", server.base_url))
        .send()
        .await
        .expect("metrics request must succeed");

    let metrics_body = metrics_response
        .text()
        .await
        .expect("metrics body must be readable");

    assert!(metrics_body.contains(
        "social_api_circuit_breaker_open_total{service=\"profile_api\"}"
    ));
    assert!(metrics_body.contains(
        "social_api_circuit_breaker_rejected_total{service=\"profile_api\"}"
    ));
    assert!(metrics_body.contains("social_api_external_calls_total"));
    assert!(metrics_body.contains("service=\"profile_api\""));
    assert!(metrics_body.contains("status=\"circuit_open\""));

    server.shutdown().await;
}

#[tokio::test]
#[serial]
async fn content_api_circuit_breaker_opens_and_rejects_following_requests() {
    let server = TestServer::spawn(|config| {
        config.post_content_api_base_url = "http://127.0.0.1:9".to_string();
        config.bonus_hunter_content_api_base_url = "http://127.0.0.1:9".to_string();
        config.top_picks_content_api_base_url = "http://127.0.0.1:9".to_string();
        config.circuit_breaker_failure_threshold = 1;
        config.circuit_breaker_open_seconds = 60;
    })
    .await;

    let first_response = server
        .client
        .post(format!("{}/v1/likes", server.base_url))
        .bearer_auth("valid-alice-token")
        .json(&json!({
            "content_type": "post",
            "content_id": "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaa1"
        }))
        .send()
        .await
        .expect("first request must succeed");

    assert_eq!(first_response.status(), StatusCode::SERVICE_UNAVAILABLE);

    let second_response = server
        .client
        .post(format!("{}/v1/likes", server.base_url))
        .bearer_auth("valid-alice-token")
        .json(&json!({
            "content_type": "post",
            "content_id": "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaa1"
        }))
        .send()
        .await
        .expect("second request must succeed");

    assert_eq!(second_response.status(), StatusCode::SERVICE_UNAVAILABLE);

    let metrics_body = fetch_metrics(&server).await;

    assert!(metrics_body.contains(
        "social_api_circuit_breaker_open_total{service=\"content_api\"}"
    ));
    assert!(metrics_body.contains(
        "social_api_circuit_breaker_rejected_total{service=\"content_api\"}"
    ));
    assert!(metrics_body.contains("social_api_external_calls_total"));
    assert!(metrics_body.contains("service=\"content_api\""));
    assert!(metrics_body.contains("status=\"circuit_open\""));

    server.shutdown().await;
}

#[tokio::test]
#[serial]
async fn profile_api_circuit_breaker_recovers_after_cooldown() {
    let mock_address = unused_socket_addr();
    let server = TestServer::spawn(|config| {
        config.profile_api_base_url = format!("http://{}", mock_address);
        config.circuit_breaker_failure_threshold = 1;
        config.circuit_breaker_open_seconds = 1;
    })
    .await;

    let first_response = server
        .client
        .get(format!("{}/v1/likes/user", server.base_url))
        .bearer_auth("valid-alice-token")
        .send()
        .await
        .expect("first request must succeed");

    assert_eq!(first_response.status(), StatusCode::SERVICE_UNAVAILABLE);

    sleep(Duration::from_millis(1100)).await;

    let profile_mock = spawn_profile_mock_server(mock_address).await;

    let second_response = server
        .client
        .get(format!("{}/v1/likes/user", server.base_url))
        .bearer_auth("valid-alice-token")
        .send()
        .await
        .expect("second request must succeed");

    assert_eq!(second_response.status(), StatusCode::OK);

    let metrics_body = fetch_metrics(&server).await;
    assert_eq!(
        metric_value(
            &metrics_body,
            "social_api_circuit_breaker_state",
            &[("service", "profile_api")],
        ),
        0.0
    );

    profile_mock.shutdown().await;
    server.shutdown().await;
}

#[tokio::test]
#[serial]
async fn content_api_circuit_breaker_recovers_after_cooldown() {
    let mock_address = unused_socket_addr();
    let server = TestServer::spawn(|config| {
        config.post_content_api_base_url = format!("http://{}", mock_address);
        config.circuit_breaker_failure_threshold = 1;
        config.circuit_breaker_open_seconds = 1;
    })
    .await;

    let first_response = server
        .client
        .post(format!("{}/v1/likes", server.base_url))
        .bearer_auth("valid-alice-token")
        .json(&json!({
            "content_type": "post",
            "content_id": "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaa1"
        }))
        .send()
        .await
        .expect("first request must succeed");

    assert_eq!(first_response.status(), StatusCode::SERVICE_UNAVAILABLE);

    sleep(Duration::from_millis(1100)).await;

    let content_mock = spawn_content_mock_server(mock_address).await;

    let second_response = server
        .client
        .post(format!("{}/v1/likes", server.base_url))
        .bearer_auth("valid-alice-token")
        .json(&json!({
            "content_type": "post",
            "content_id": "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaa1"
        }))
        .send()
        .await
        .expect("second request must succeed");

    assert_eq!(second_response.status(), StatusCode::CREATED);

    let metrics_body = fetch_metrics(&server).await;
    assert_eq!(
        metric_value(
            &metrics_body,
            "social_api_circuit_breaker_state",
            &[("service", "content_api")],
        ),
        0.0
    );

    content_mock.shutdown().await;
    server.shutdown().await;
}

#[tokio::test]
#[serial]
async fn top_likes_returns_items_sorted_by_count() {
    let server = TestServer::spawn(|_| {}).await;

    create_like(
        &server,
        "valid-alice-token",
        "post",
        "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaa1",
    )
    .await;
    create_like(
        &server,
        "valid-bob-token",
        "post",
        "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaa1",
    )
    .await;
    create_like(
        &server,
        "valid-charlie-token",
        "post",
        "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaa1",
    )
    .await;

    create_like(
        &server,
        "valid-alice-token",
        "bonus_hunter",
        "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbb1",
    )
    .await;
    create_like(
        &server,
        "valid-bob-token",
        "bonus_hunter",
        "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbb1",
    )
    .await;

    let response = server
        .client
        .get(format!("{}/v1/likes/top?window=all&limit=10", server.base_url))
        .send()
        .await
        .expect("top request must succeed");

    assert_eq!(response.status(), StatusCode::OK);

    let body: Value = response.json().await.expect("response must be valid json");
    assert_eq!(body["results"][0]["content_type"], "post");
    assert_eq!(
        body["results"][0]["content_id"],
        "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaa1"
    );
    assert_eq!(body["results"][0]["count"], 3);
    assert_eq!(body["results"][1]["content_type"], "bonus_hunter");
    assert_eq!(
        body["results"][1]["content_id"],
        "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbb1"
    );
    assert_eq!(body["results"][1]["count"], 2);

    server.shutdown().await;
}

#[tokio::test]
#[serial]
async fn likes_stream_emits_snapshot_event() {
    let server = TestServer::spawn(|_| {}).await;

    create_like(
        &server,
        "valid-alice-token",
        "post",
        "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaa1",
    )
    .await;
    create_like(
        &server,
        "valid-bob-token",
        "post",
        "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaa1",
    )
    .await;

    let response = server
        .client
        .get(format!(
            "{}/v1/likes/stream?window=all&limit=5",
            server.base_url
        ))
        .send()
        .await
        .expect("stream request must succeed");

    assert_eq!(response.status(), StatusCode::OK);

    let first_chunk = tokio::time::timeout(Duration::from_secs(2), async move {
        let mut response = response;
        response.chunk().await
    })
    .await
    .expect("stream should emit quickly")
    .expect("chunk read should succeed")
    .expect("stream should emit at least one chunk");

    let chunk = String::from_utf8(first_chunk.to_vec()).expect("chunk must be utf8");
    assert!(chunk.contains("event: snapshot"));
    assert!(chunk.contains("\"window\":\"all\""));
    assert!(chunk.contains("\"content_type\":\"post\""));

    server.shutdown().await;
}

#[tokio::test]
#[serial]
async fn count_falls_back_to_postgres_when_redis_is_unavailable() {
    let server = TestServer::spawn(|config| {
        config.redis_url = "redis://127.0.0.1:9/".to_string();
    })
    .await;

    insert_like_directly(
        &server.database_url,
        "11111111-1111-1111-1111-111111111111",
        "post",
        "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaa1",
    )
    .await;

    let metrics_before = fetch_metrics(&server).await;
    let fail_open_before = metric_value_or_zero(
        &metrics_before,
        "social_api_rate_limit_fail_open_total",
        &[("scope", "read_ip")],
    );

    let response = server
        .client
        .get(format!(
            "{}/v1/likes/post/aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaa1/count",
            server.base_url
        ))
        .send()
        .await
        .expect("count request must succeed");

    assert_eq!(response.status(), StatusCode::OK);

    let body: Value = response.json().await.expect("response must be valid json");
    assert_eq!(body["count"], 1);

    let metrics_after = fetch_metrics(&server).await;
    assert!(
        metric_value_or_zero(
            &metrics_after,
            "social_api_rate_limit_fail_open_total",
            &[("scope", "read_ip")],
        ) >= fail_open_before + 1.0
    );

    server.shutdown().await;
}

#[tokio::test]
#[serial]
async fn read_rate_limit_returns_429_when_exceeded() {
    let server = TestServer::spawn(|config| {
        config.read_rate_limit_per_minute = 1;
    })
    .await;

    let metrics_before = fetch_metrics(&server).await;
    let allowed_before = metric_value_or_zero(
        &metrics_before,
        "social_api_rate_limit_allowed_total",
        &[("scope", "read_ip")],
    );
    let rejected_before = metric_value_or_zero(
        &metrics_before,
        "social_api_rate_limit_rejected_total",
        &[("scope", "read_ip")],
    );

    let first_response = server
        .client
        .get(format!(
            "{}/v1/likes/post/aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaa1/count",
            server.base_url
        ))
        .header("x-forwarded-for", "203.0.113.10")
        .send()
        .await
        .expect("first read request must succeed");

    assert_eq!(first_response.status(), StatusCode::OK);

    let second_response = server
        .client
        .get(format!(
            "{}/v1/likes/post/aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaa1/count",
            server.base_url
        ))
        .header("x-forwarded-for", "203.0.113.10")
        .send()
        .await
        .expect("second read request must succeed");

    assert_eq!(second_response.status(), StatusCode::TOO_MANY_REQUESTS);

    let body: Value = second_response.json().await.expect("response must be valid json");
    assert_eq!(body["error"]["code"], "RATE_LIMITED");

    let metrics_after = fetch_metrics(&server).await;
    assert!(
        metric_value_or_zero(
            &metrics_after,
            "social_api_rate_limit_allowed_total",
            &[("scope", "read_ip")],
        ) >= allowed_before + 1.0
    );
    assert!(
        metric_value_or_zero(
            &metrics_after,
            "social_api_rate_limit_rejected_total",
            &[("scope", "read_ip")],
        ) >= rejected_before + 1.0
    );

    server.shutdown().await;
}

#[tokio::test]
#[serial]
async fn sse_metrics_track_connections_and_events() {
    let server = TestServer::spawn(|_| {}).await;

    create_like(
        &server,
        "valid-alice-token",
        "post",
        "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaa1",
    )
    .await;

    let response = server
        .client
        .get(format!(
            "{}/v1/likes/stream?window=all&limit=5",
            server.base_url
        ))
        .send()
        .await
        .expect("stream request must succeed");

    assert_eq!(response.status(), StatusCode::OK);

    let mut response = response;
    let first_chunk = tokio::time::timeout(Duration::from_secs(2), async {
        response.chunk().await
    })
    .await
    .expect("stream should emit quickly")
    .expect("chunk read should succeed")
    .expect("stream should emit at least one chunk");

    let chunk = String::from_utf8(first_chunk.to_vec()).expect("chunk must be utf8");
    assert!(chunk.contains("event: snapshot"));

    let metrics_while_open = server
        .client
        .get(format!("{}/metrics", server.base_url))
        .send()
        .await
        .expect("metrics request must succeed")
        .text()
        .await
        .expect("metrics body must be readable");

    assert!(
        metric_value(
            &metrics_while_open,
            "social_api_sse_connections_total",
            &[("stream", "top_likes")],
        ) >= 1.0
    );
    assert_eq!(
        metric_value(
            &metrics_while_open,
            "social_api_sse_connections_active",
            &[("stream", "top_likes")],
        ),
        1.0
    );
    assert!(
        metric_value(
            &metrics_while_open,
            "social_api_sse_events_sent_total",
            &[("stream", "top_likes"), ("event", "snapshot")],
        ) >= 1.0
    );

    drop(response);
    sleep(Duration::from_millis(50)).await;

    let metrics_after_close = server
        .client
        .get(format!("{}/metrics", server.base_url))
        .send()
        .await
        .expect("metrics request must succeed")
        .text()
        .await
        .expect("metrics body must be readable");

    assert_eq!(
        metric_value(
            &metrics_after_close,
            "social_api_sse_connections_active",
            &[("stream", "top_likes")],
        ),
        0.0
    );
    assert!(
        metric_value(
            &metrics_after_close,
            "social_api_sse_disconnects_total",
            &[("stream", "top_likes")],
        ) >= 1.0
    );

    server.shutdown().await;
}

#[tokio::test]
#[serial]
async fn graceful_shutdown_stops_accepting_new_requests() {
    let mut server = TestServer::spawn(|_| {}).await;

    let health_response = server
        .client
        .get(format!("{}/health/live", server.base_url))
        .send()
        .await
        .expect("health request must succeed before shutdown");

    assert_eq!(health_response.status(), StatusCode::OK);

    server.trigger_shutdown();
    server.wait_for_shutdown().await;

    let result = server
        .client
        .get(format!("{}/health/live", server.base_url))
        .send()
        .await;

    assert!(result.is_err(), "server should refuse new connections after shutdown");

    server.cleanup().await;
}

#[tokio::test]
#[serial]
async fn graceful_shutdown_closes_sse_connections() {
    let mut server = TestServer::spawn(|_| {}).await;

    create_like(
        &server,
        "valid-alice-token",
        "post",
        "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaa1",
    )
    .await;

    let response = server
        .client
        .get(format!(
            "{}/v1/likes/stream?window=all&limit=5",
            server.base_url
        ))
        .send()
        .await
        .expect("stream request must succeed");

    assert_eq!(response.status(), StatusCode::OK);

    let mut response = response;
    let first_chunk = tokio::time::timeout(Duration::from_secs(2), async {
        response.chunk().await
    })
    .await
    .expect("stream should emit quickly")
    .expect("chunk read should succeed")
    .expect("stream should emit at least one chunk");

    let chunk = String::from_utf8(first_chunk.to_vec()).expect("chunk must be utf8");
    assert!(chunk.contains("event: snapshot"));

    server.trigger_shutdown();
    server.wait_for_shutdown().await;

    let next_chunk = tokio::time::timeout(Duration::from_secs(2), async {
        response.chunk().await
    })
    .await
    .expect("stream should close promptly after shutdown")
    .expect("chunk read after shutdown should succeed");

    assert!(
        next_chunk.is_none(),
        "stream should be closed after graceful shutdown"
    );

    server.cleanup().await;
}

struct TestServer {
    base_url: String,
    client: reqwest::Client,
    shutdown_tx: Option<oneshot::Sender<()>>,
    server_handle: Option<JoinHandle<()>>,
    shutdown_signal: crate::shutdown::ShutdownSignal,
    database_url: String,
    redis_url: String,
}

struct AuxiliaryServer {
    shutdown_tx: Option<oneshot::Sender<()>>,
    handle: Option<JoinHandle<()>>,
}

impl TestServer {
    async fn spawn(override_config: impl FnOnce(&mut ServiceConfig)) -> Self {
        init_test_runtime();

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("test listener must bind");
        let address = listener.local_addr().expect("listener must expose address");
        let base_url = format!("http://{}", address);

        let mut config = base_test_config(address);
        override_config(&mut config);

        cleanup_database(&config.database_url).await;
        let local_redis_url = env::var("REDIS_URL").expect("REDIS_URL must be set for tests");
        if config.redis_url == local_redis_url {
            cleanup_redis(&config.redis_url).await;
        }

        let mut bootstrap_config = base_test_config(address);
        bootstrap_config.write_rate_limit_per_minute = config.write_rate_limit_per_minute;
        bootstrap_config.read_rate_limit_per_minute = config.read_rate_limit_per_minute;
        bootstrap_config.circuit_breaker_failure_threshold = config.circuit_breaker_failure_threshold;
        bootstrap_config.circuit_breaker_open_seconds = config.circuit_breaker_open_seconds;
        bootstrap_config.profile_api_base_url = config.profile_api_base_url.clone();
        bootstrap_config.post_content_api_base_url = config.post_content_api_base_url.clone();
        bootstrap_config.bonus_hunter_content_api_base_url =
            config.bonus_hunter_content_api_base_url.clone();
        bootstrap_config.top_picks_content_api_base_url =
            config.top_picks_content_api_base_url.clone();

        let mut app_state = build_app_state(&bootstrap_config).await;
        let shutdown_signal = app_state.shutdown_signal.clone();
        if config.redis_url != local_redis_url {
            app_state.redis_client = redis::Client::open(config.redis_url.clone())
                .expect("test redis override must be a valid url");
        }
        let app = build_app(app_state);

        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let server_handle = tokio::spawn(async move {
            axum::serve(
                listener,
                app.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .with_graceful_shutdown(async {
                let _ = shutdown_rx.await;
            })
            .await
            .expect("test server must run");
        });

        sleep(Duration::from_millis(50)).await;

        Self {
            base_url,
            client: reqwest::Client::new(),
            shutdown_tx: Some(shutdown_tx),
            server_handle: Some(server_handle),
            shutdown_signal,
            database_url: config.database_url,
            redis_url: config.redis_url,
        }
    }

    async fn shutdown(mut self) {
        self.trigger_shutdown();
        self.wait_for_shutdown().await;
        self.cleanup().await;
    }

    fn trigger_shutdown(&mut self) {
        self.shutdown_signal.trigger();
        if let Some(shutdown_tx) = self.shutdown_tx.take() {
            let _ = shutdown_tx.send(());
        }
    }

    async fn wait_for_shutdown(&mut self) {
        if let Some(server_handle) = self.server_handle.take() {
            server_handle
                .await
                .expect("test server task must finish cleanly");
        }
    }

    async fn cleanup(self) {
        cleanup_database(&self.database_url).await;
        if self.redis_url == env::var("REDIS_URL").expect("REDIS_URL must be set for tests") {
            cleanup_redis(&self.redis_url).await;
        }
    }
}

impl AuxiliaryServer {
    async fn shutdown(mut self) {
        if let Some(shutdown_tx) = self.shutdown_tx.take() {
            let _ = shutdown_tx.send(());
        }

        if let Some(handle) = self.handle.take() {
            handle
                .await
                .expect("auxiliary server task must finish cleanly");
        }
    }
}

fn init_test_runtime() {
    TEST_INIT.call_once(|| {
        init_tracing();
        init_metrics();
    });
}

fn base_test_config(address: SocketAddr) -> ServiceConfig {
    ServiceConfig {
        host: "127.0.0.1".to_string(),
        port: address.port(),
        database_url: env::var("DATABASE_URL").expect("DATABASE_URL must be set for tests"),
        redis_url: env::var("REDIS_URL").expect("REDIS_URL must be set for tests"),
        write_rate_limit_per_minute: 30,
        read_rate_limit_per_minute: 1000,
        circuit_breaker_failure_threshold: 3,
        circuit_breaker_open_seconds: 30,
        profile_api_base_url: format!("http://{}", address),
        post_content_api_base_url: format!("http://{}", address),
        bonus_hunter_content_api_base_url: format!("http://{}", address),
        top_picks_content_api_base_url: format!("http://{}", address),
    }
}

fn unused_socket_addr() -> SocketAddr {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")
        .expect("temporary listener must bind to discover a free port");
    let address = listener
        .local_addr()
        .expect("temporary listener must expose its address");
    drop(listener);
    address
}

async fn spawn_profile_mock_server(address: SocketAddr) -> AuxiliaryServer {
    let app = Router::new().route(
        "/v1/auth/validate",
        get(|| async {
            Json(json!({
                "user_id": "11111111-1111-1111-1111-111111111111",
                "display_name": "Alice Test"
            }))
        }),
    );

    spawn_auxiliary_server(address, app).await
}

async fn spawn_content_mock_server(address: SocketAddr) -> AuxiliaryServer {
    let app = Router::new().route(
        "/v1/{content_type}/{content_id}",
        get(|Path((content_type, content_id)): Path<(String, String)>| async move {
            Json(json!({
                "content_type": content_type,
                "content_id": content_id,
                "exists": true
            }))
        }),
    );

    spawn_auxiliary_server(address, app).await
}

async fn spawn_auxiliary_server(address: SocketAddr, app: Router) -> AuxiliaryServer {
    let listener = tokio::net::TcpListener::bind(address)
        .await
        .expect("auxiliary listener must bind");
    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let handle = tokio::spawn(async move {
        axum::serve(listener, app.into_make_service())
            .with_graceful_shutdown(async {
                let _ = shutdown_rx.await;
            })
            .await
            .expect("auxiliary server must run");
    });

    sleep(Duration::from_millis(50)).await;

    AuxiliaryServer {
        shutdown_tx: Some(shutdown_tx),
        handle: Some(handle),
    }
}

async fn cleanup_database(database_url: &str) {
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect(database_url)
        .await
        .expect("test database connection must work");

    sqlx::query("TRUNCATE likes, like_counts")
        .execute(&pool)
        .await
        .expect("test database cleanup must succeed");
}

async fn cleanup_redis(redis_url: &str) {
    let client = redis::Client::open(redis_url).expect("test redis url must be valid");
    let mut connection = client
        .get_multiplexed_async_connection()
        .await
        .expect("test redis connection must work");

    for pattern in ["likes:*", "rate_limit:*"] {
        let keys: Vec<String> = redis::cmd("KEYS")
            .arg(pattern)
            .query_async(&mut connection)
            .await
            .expect("redis KEYS must succeed");

        if !keys.is_empty() {
            let _: i64 = redis::cmd("DEL")
                .arg(keys)
                .query_async(&mut connection)
                .await
                .expect("redis DEL must succeed");
        }
    }
}

async fn create_like(server: &TestServer, token: &str, content_type: &str, content_id: &str) {
    let response = server
        .client
        .post(format!("{}/v1/likes", server.base_url))
        .bearer_auth(token)
        .json(&json!({
            "content_type": content_type,
            "content_id": content_id
        }))
        .send()
        .await
        .expect("create like request must succeed");

    assert!(
        response.status() == StatusCode::CREATED || response.status() == StatusCode::OK,
        "unexpected like response status: {}",
        response.status()
    );
}

async fn fetch_metrics(server: &TestServer) -> String {
    server
        .client
        .get(format!("{}/metrics", server.base_url))
        .send()
        .await
        .expect("metrics request must succeed")
        .text()
        .await
        .expect("metrics body must be readable")
}

fn metric_value(metrics_body: &str, metric_name: &str, labels: &[(&str, &str)]) -> f64 {
    metrics_body
        .lines()
        .find_map(|line| {
            if !line.starts_with(metric_name) {
                return None;
            }

            if !labels
                .iter()
                .all(|(key, value)| line.contains(&format!("{key}=\"{value}\"")))
            {
                return None;
            }

            line.split_whitespace().last()?.parse::<f64>().ok()
        })
        .unwrap_or_else(|| panic!("metric {metric_name} with labels {:?} not found", labels))
}

fn metric_value_or_zero(metrics_body: &str, metric_name: &str, labels: &[(&str, &str)]) -> f64 {
    metrics_body
        .lines()
        .find_map(|line| {
            if !line.starts_with(metric_name) {
                return None;
            }

            if !labels
                .iter()
                .all(|(key, value)| line.contains(&format!("{key}=\"{value}\"")))
            {
                return None;
            }

            line.split_whitespace().last()?.parse::<f64>().ok()
        })
        .unwrap_or(0.0)
}

async fn insert_like_directly(
    database_url: &str,
    user_id: &str,
    content_type: &str,
    content_id: &str,
) {
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect(database_url)
        .await
        .expect("test database connection must work");

    let mut transaction = pool
        .begin()
        .await
        .expect("test transaction must start");

    sqlx::query(
        r#"
        INSERT INTO likes (user_id, content_type, content_id)
        VALUES ($1, $2, $3)
        ON CONFLICT (user_id, content_type, content_id) DO NOTHING
        "#,
    )
    .bind(user_id)
    .bind(content_type)
    .bind(content_id)
    .execute(&mut *transaction)
    .await
    .expect("test like insert must succeed");

    sqlx::query(
        r#"
        INSERT INTO like_counts (content_type, content_id, like_count)
        VALUES ($1, $2, 1)
        ON CONFLICT (content_type, content_id)
        DO UPDATE SET like_count = 1
        "#,
    )
    .bind(content_type)
    .bind(content_id)
    .execute(&mut *transaction)
    .await
    .expect("test like count upsert must succeed");

    transaction
        .commit()
        .await
        .expect("test transaction must commit");
}
