use axum::{
    extract::State,
    http::{header, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
};
use crate::app_state::AppState;
use once_cell::sync::Lazy;
use prometheus::{
    Encoder, HistogramOpts, HistogramVec, IntCounterVec, IntGaugeVec, Opts, Registry,
    TextEncoder,
};

static REGISTRY: Lazy<Registry> = Lazy::new(Registry::new);

static HTTP_REQUESTS_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    IntCounterVec::new(
        Opts::new(
            "social_api_http_requests_total",
            "Total number of HTTP requests",
        ),
        &["method", "path", "status"],
    )
    .expect("http requests counter must be valid")
});

static HTTP_REQUEST_DURATION_SECONDS: Lazy<HistogramVec> = Lazy::new(|| {
    HistogramVec::new(
        HistogramOpts::new(
            "social_api_http_request_duration_seconds",
            "HTTP request duration in seconds",
        ),
        &["method", "path"],
    )
    .expect("http request histogram must be valid")
});

static CACHE_OPERATIONS_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    IntCounterVec::new(
        Opts::new(
            "social_api_cache_operations_total",
            "Total number of cache operations",
        ),
        &["operation", "result"],
    )
    .expect("cache operations counter must be valid")
});

static EXTERNAL_CALLS_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    IntCounterVec::new(
        Opts::new(
            "social_api_external_calls_total",
            "Total number of external service calls",
        ),
        &["service", "method", "status"],
    )
    .expect("external calls counter must be valid")
});

static EXTERNAL_CALL_DURATION_SECONDS: Lazy<HistogramVec> = Lazy::new(|| {
    HistogramVec::new(
        HistogramOpts::new(
            "social_api_external_call_duration_seconds",
            "External service call duration in seconds",
        ),
        &["service", "method"],
    )
    .expect("external call histogram must be valid")
});

static LIKES_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    IntCounterVec::new(
        Opts::new(
            "social_api_likes_total",
            "Total number of effective like and unlike operations",
        ),
        &["content_type", "operation"],
    )
    .expect("likes total counter must be valid")
});

static CIRCUIT_BREAKER_OPEN_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    IntCounterVec::new(
        Opts::new(
            "social_api_circuit_breaker_open_total",
            "Total number of circuit breaker openings",
        ),
        &["service"],
    )
    .expect("circuit breaker open counter must be valid")
});

static CIRCUIT_BREAKER_REJECTED_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    IntCounterVec::new(
        Opts::new(
            "social_api_circuit_breaker_rejected_total",
            "Total number of requests rejected because the circuit breaker is open",
        ),
        &["service"],
    )
    .expect("circuit breaker rejected counter must be valid")
});

static CIRCUIT_BREAKER_STATE: Lazy<IntGaugeVec> = Lazy::new(|| {
    IntGaugeVec::new(
        Opts::new(
            "social_api_circuit_breaker_state",
            "Current circuit breaker state, where 0 is closed, 1 is half-open, and 2 is open",
        ),
        &["service"],
    )
    .expect("circuit breaker state gauge must be valid")
});

static SSE_CONNECTIONS_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    IntCounterVec::new(
        Opts::new(
            "social_api_sse_connections_total",
            "Total number of SSE connections opened",
        ),
        &["stream"],
    )
    .expect("sse connections counter must be valid")
});

static SSE_CONNECTIONS_ACTIVE: Lazy<IntGaugeVec> = Lazy::new(|| {
    IntGaugeVec::new(
        Opts::new(
            "social_api_sse_connections_active",
            "Current number of active SSE connections",
        ),
        &["stream"],
    )
    .expect("sse active gauge must be valid")
});

static SSE_EVENTS_SENT_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    IntCounterVec::new(
        Opts::new(
            "social_api_sse_events_sent_total",
            "Total number of SSE events sent",
        ),
        &["stream", "event"],
    )
    .expect("sse events counter must be valid")
});

static SSE_DISCONNECTS_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    IntCounterVec::new(
        Opts::new(
            "social_api_sse_disconnects_total",
            "Total number of SSE connections closed",
        ),
        &["stream"],
    )
    .expect("sse disconnects counter must be valid")
});

static RATE_LIMIT_ALLOWED_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    IntCounterVec::new(
        Opts::new(
            "social_api_rate_limit_allowed_total",
            "Total number of requests allowed by the rate limiter",
        ),
        &["scope"],
    )
    .expect("rate limit allowed counter must be valid")
});

static RATE_LIMIT_REJECTED_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    IntCounterVec::new(
        Opts::new(
            "social_api_rate_limit_rejected_total",
            "Total number of requests rejected by the rate limiter",
        ),
        &["scope"],
    )
    .expect("rate limit rejected counter must be valid")
});

static RATE_LIMIT_FAIL_OPEN_TOTAL: Lazy<IntCounterVec> = Lazy::new(|| {
    IntCounterVec::new(
        Opts::new(
            "social_api_rate_limit_fail_open_total",
            "Total number of requests allowed because the rate limiter failed open",
        ),
        &["scope"],
    )
    .expect("rate limit fail-open counter must be valid")
});

static DB_POOL_CONNECTIONS: Lazy<IntGaugeVec> = Lazy::new(|| {
    IntGaugeVec::new(
        Opts::new(
            "social_api_db_pool_connections",
            "Current number of database pool connections by state",
        ),
        &["state"],
    )
    .expect("db pool connections gauge must be valid")
});

pub fn init_metrics() {
    REGISTRY
        .register(Box::new(HTTP_REQUESTS_TOTAL.clone()))
        .expect("http requests counter must register");
    REGISTRY
        .register(Box::new(HTTP_REQUEST_DURATION_SECONDS.clone()))
        .expect("http request histogram must register");
    REGISTRY
        .register(Box::new(CACHE_OPERATIONS_TOTAL.clone()))
        .expect("cache operations counter must register");
    REGISTRY
        .register(Box::new(EXTERNAL_CALLS_TOTAL.clone()))
        .expect("external calls counter must register");
    REGISTRY
        .register(Box::new(EXTERNAL_CALL_DURATION_SECONDS.clone()))
        .expect("external call histogram must register");
    REGISTRY
        .register(Box::new(LIKES_TOTAL.clone()))
        .expect("likes total counter must register");
    REGISTRY
        .register(Box::new(CIRCUIT_BREAKER_OPEN_TOTAL.clone()))
        .expect("circuit breaker open counter must register");
    REGISTRY
        .register(Box::new(CIRCUIT_BREAKER_REJECTED_TOTAL.clone()))
        .expect("circuit breaker rejected counter must register");
    REGISTRY
        .register(Box::new(CIRCUIT_BREAKER_STATE.clone()))
        .expect("circuit breaker state gauge must register");
    REGISTRY
        .register(Box::new(SSE_CONNECTIONS_TOTAL.clone()))
        .expect("sse connections counter must register");
    REGISTRY
        .register(Box::new(SSE_CONNECTIONS_ACTIVE.clone()))
        .expect("sse active gauge must register");
    REGISTRY
        .register(Box::new(SSE_EVENTS_SENT_TOTAL.clone()))
        .expect("sse events counter must register");
    REGISTRY
        .register(Box::new(SSE_DISCONNECTS_TOTAL.clone()))
        .expect("sse disconnects counter must register");
    REGISTRY
        .register(Box::new(RATE_LIMIT_ALLOWED_TOTAL.clone()))
        .expect("rate limit allowed counter must register");
    REGISTRY
        .register(Box::new(RATE_LIMIT_REJECTED_TOTAL.clone()))
        .expect("rate limit rejected counter must register");
    REGISTRY
        .register(Box::new(RATE_LIMIT_FAIL_OPEN_TOTAL.clone()))
        .expect("rate limit fail-open counter must register");
    REGISTRY
        .register(Box::new(DB_POOL_CONNECTIONS.clone()))
        .expect("db pool connections gauge must register");
}

pub fn record_http_request(method: &str, path: &str, status: u16, latency_seconds: f64) {
    let status = status.to_string();

    HTTP_REQUESTS_TOTAL
        .with_label_values(&[method, path, &status])
        .inc();
    HTTP_REQUEST_DURATION_SECONDS
        .with_label_values(&[method, path])
        .observe(latency_seconds);
}

pub fn record_cache_operation(operation: &str, result: &str) {
    CACHE_OPERATIONS_TOTAL
        .with_label_values(&[operation, result])
        .inc();
}

pub fn record_external_call(service: &str, method: &str, status: &str, latency_seconds: f64) {
    EXTERNAL_CALLS_TOTAL
        .with_label_values(&[service, method, status])
        .inc();
    EXTERNAL_CALL_DURATION_SECONDS
        .with_label_values(&[service, method])
        .observe(latency_seconds);
}

pub fn record_like_operation(content_type: &str, operation: &str) {
    LIKES_TOTAL
        .with_label_values(&[content_type, operation])
        .inc();
}

pub fn record_circuit_breaker_open(service: &str) {
    CIRCUIT_BREAKER_OPEN_TOTAL
        .with_label_values(&[service])
        .inc();
}

pub fn record_circuit_breaker_rejected(service: &str) {
    CIRCUIT_BREAKER_REJECTED_TOTAL
        .with_label_values(&[service])
        .inc();
}

pub fn set_circuit_breaker_state(service: &str, state: i64) {
    CIRCUIT_BREAKER_STATE.with_label_values(&[service]).set(state);
}

pub fn record_sse_connection_open(stream: &str) {
    SSE_CONNECTIONS_TOTAL.with_label_values(&[stream]).inc();
    SSE_CONNECTIONS_ACTIVE
        .with_label_values(&[stream])
        .inc();
}

pub fn record_sse_connection_close(stream: &str) {
    SSE_DISCONNECTS_TOTAL.with_label_values(&[stream]).inc();
    SSE_CONNECTIONS_ACTIVE
        .with_label_values(&[stream])
        .dec();
}

pub fn record_sse_event_sent(stream: &str, event: &str) {
    SSE_EVENTS_SENT_TOTAL
        .with_label_values(&[stream, event])
        .inc();
}

pub fn record_rate_limit_allowed(scope: &str) {
    RATE_LIMIT_ALLOWED_TOTAL.with_label_values(&[scope]).inc();
}

pub fn record_rate_limit_rejected(scope: &str) {
    RATE_LIMIT_REJECTED_TOTAL.with_label_values(&[scope]).inc();
}

pub fn record_rate_limit_fail_open(scope: &str) {
    RATE_LIMIT_FAIL_OPEN_TOTAL.with_label_values(&[scope]).inc();
}

fn update_db_pool_metrics(state: &AppState) {
    let write_total = state.db_pool.size() as i64;
    let write_idle = state.db_pool.num_idle() as i64;
    let write_max = state.db_pool.options().get_max_connections() as i64;
    let read_total = state.read_db_pool.size() as i64;
    let read_idle = state.read_db_pool.num_idle() as i64;
    let read_max = state.read_db_pool.options().get_max_connections() as i64;

    DB_POOL_CONNECTIONS
        .with_label_values(&["max"])
        .set(write_max + read_max);
    DB_POOL_CONNECTIONS
        .with_label_values(&["idle"])
        .set(write_idle + read_idle);
    DB_POOL_CONNECTIONS
        .with_label_values(&["active"])
        .set(write_total.saturating_sub(write_idle) + read_total.saturating_sub(read_idle));
}

pub async fn metrics_handler(State(state): State<AppState>) -> Response {
    update_db_pool_metrics(&state);

    let metric_families = REGISTRY.gather();
    let mut buffer = Vec::new();
    let encoder = TextEncoder::new();

    if encoder.encode(&metric_families, &mut buffer).is_err() {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    let mut response = buffer.into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(encoder.format_type())
            .expect("prometheus content-type must be valid"),
    );
    response
}
