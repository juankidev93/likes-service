use axum::{
    body::{to_bytes, Body},
    extract::{MatchedPath, Request},
    http::{header, header::HeaderName, HeaderValue},
    middleware::Next,
    response::Response,
};
use serde_json::Value;
use std::time::Instant;
use tracing::info;
use uuid::Uuid;

use crate::metrics::record_http_request;

const REQUEST_ID_HEADER: &str = "x-request-id";
const SERVICE_NAME: &str = "likes_service";

#[allow(dead_code)]
#[derive(Clone, Debug)]
pub struct RequestId(pub String);

#[derive(Clone, Debug)]
pub struct LoggedUserId(pub String);

#[derive(Clone, Debug)]
pub struct ErrorLogContext {
    pub error_type: &'static str,
    pub error_message: String,
    pub stack_trace: Option<String>,
}

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
    let user_id = response
        .extensions()
        .get::<LoggedUserId>()
        .map(|value| value.0.clone());
    let error_context = response.extensions().get::<ErrorLogContext>().cloned();

    response.headers_mut().insert(
        HeaderName::from_static(REQUEST_ID_HEADER),
        HeaderValue::from_str(&request_id).expect("request_id must be a valid header value"),
    );

    response = inject_request_id_into_error_body(response, &request_id).await;

    record_http_request(&method, &path, status, latency_seconds);

    match (status >= 500, user_id.as_deref(), error_context) {
        (true, Some(user_id), Some(error_context)) => tracing::error!(
            service = SERVICE_NAME,
            method = method,
            path = path,
            status_code = status,
            latency_ms = latency_ms,
            request_id = request_id,
            user_id = user_id,
            error_type = error_context.error_type,
            error_message = error_context.error_message,
            stack_trace = error_context.stack_trace.unwrap_or_else(|| "unavailable".to_string()),
            "request failed"
        ),
        (true, None, Some(error_context)) => tracing::error!(
            service = SERVICE_NAME,
            method = method,
            path = path,
            status_code = status,
            latency_ms = latency_ms,
            request_id = request_id,
            error_type = error_context.error_type,
            error_message = error_context.error_message,
            stack_trace = error_context.stack_trace.unwrap_or_else(|| "unavailable".to_string()),
            "request failed"
        ),
        (_, Some(user_id), _) => info!(
            service = SERVICE_NAME,
            method = method,
            path = path,
            status_code = status,
            latency_ms = latency_ms,
            request_id = request_id,
            user_id = user_id,
            "request completed"
        ),
        _ => info!(
            service = SERVICE_NAME,
            method = method,
            path = path,
            status_code = status,
            latency_ms = latency_ms,
            request_id = request_id,
            "request completed"
        ),
    };

    response
}

async fn inject_request_id_into_error_body(response: Response, request_id: &str) -> Response {
    if !(response.status().is_client_error() || response.status().is_server_error()) {
        return response;
    }

    let is_json = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.starts_with("application/json"));

    if !is_json {
        return response;
    }

    let (parts, body) = response.into_parts();
    let body_bytes = match to_bytes(body, usize::MAX).await {
        Ok(bytes) => bytes,
        Err(_) => return Response::from_parts(parts, Body::empty()),
    };

    let mut body_value = match serde_json::from_slice::<Value>(&body_bytes) {
        Ok(value) => value,
        Err(_) => return Response::from_parts(parts, Body::from(body_bytes)),
    };

    let Some(error_object) = body_value.get_mut("error").and_then(Value::as_object_mut) else {
        return Response::from_parts(parts, Body::from(body_bytes));
    };

    error_object.insert(
        "request_id".to_string(),
        Value::String(request_id.to_string()),
    );

    let encoded_body = match serde_json::to_vec(&body_value) {
        Ok(body) => body,
        Err(_) => return Response::from_parts(parts, Body::from(body_bytes)),
    };

    Response::from_parts(parts, Body::from(encoded_body))
}
