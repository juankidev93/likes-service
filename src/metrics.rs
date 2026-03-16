use axum::{
    http::{header, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
};
use once_cell::sync::Lazy;
use prometheus::{
    Encoder, HistogramOpts, HistogramVec, IntCounterVec, Opts, Registry, TextEncoder,
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

pub async fn metrics_handler() -> Response {
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
