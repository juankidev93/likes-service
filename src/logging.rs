use axum::{
    extract::{MatchedPath, Request},
    http::{header::HeaderName, HeaderValue},
    middleware::Next,
    response::Response,
};
use std::time::Instant;
use tracing::info;
use uuid::Uuid;

use crate::metrics::record_http_request;

const REQUEST_ID_HEADER: &str = "x-request-id";

#[allow(dead_code)]
#[derive(Clone, Debug)]
pub struct RequestId(pub String);

pub fn init_tracing() {
    tracing_subscriber::fmt()
        .json()
        .with_current_span(false)
        .with_span_list(false)
        .init();
}

pub async fn request_logging_middleware(mut request: Request, next: Next) -> Response {
    let method = request.method().to_string();
    let path = request
        .extensions()
        .get::<MatchedPath>()
        .map(MatchedPath::as_str)
        .unwrap_or_else(|| request.uri().path())
        .to_string();
    let request_id = request
        .headers()
        .get(REQUEST_ID_HEADER)
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.trim().is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| Uuid::new_v4().to_string());

    request
        .extensions_mut()
        .insert(RequestId(request_id.clone()));

    let start = Instant::now();
    let mut response = next.run(request).await;
    let status = response.status().as_u16();
    let elapsed = start.elapsed();
    let latency_ms = elapsed.as_millis();
    let latency_seconds = elapsed.as_secs_f64();

    response.headers_mut().insert(
        HeaderName::from_static(REQUEST_ID_HEADER),
        HeaderValue::from_str(&request_id).expect("request_id must be a valid header value"),
    );

    record_http_request(&method, &path, status, latency_seconds);

    info!(
        method = method,
        path = path,
        status_code = status,
        latency_ms = latency_ms,
        request_id = request_id,
        "request completed"
    );

    response
}
