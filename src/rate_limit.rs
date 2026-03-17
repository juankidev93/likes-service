use crate::app_state::AppState;
use crate::auth_middleware::authenticate_headers;
use crate::error::{set_rate_limit_headers, AppError};
use crate::metrics::{
    record_rate_limit_allowed, record_rate_limit_fail_open, record_rate_limit_rejected,
};
use axum::{
    extract::{ConnectInfo, Request},
    middleware::Next,
    response::Response,
};
use redis::AsyncCommands;
use std::net::SocketAddr;
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::warn;

const WRITE_SCOPE: &str = "write_user";
const READ_SCOPE: &str = "read_ip";

pub async fn require_write_auth_and_rate_limit(
    axum::extract::State(state): axum::extract::State<AppState>,
    mut request: Request,
    next: Next,
) -> Response {
    let authenticated_user = match authenticate_headers(&state, request.headers()).await {
        Ok(authenticated_user) => authenticated_user,
        Err(response) => return response,
    };

    let now_seconds = current_unix_timestamp();
    let limit = state.write_rate_limit_per_minute;
    let (key, reset_epoch_seconds, retry_after_seconds) =
        rate_limit_window_key("write", &authenticated_user.user_id, now_seconds);

    let rate_limit_state = match increment_and_read_rate_limit(&state, &key, retry_after_seconds).await {
        Ok(state) => state,
        Err(error) => {
            record_rate_limit_fail_open(WRITE_SCOPE);
            warn!(error = %error, "redis unavailable for write rate limiting, allowing request");
            request.extensions_mut().insert(authenticated_user);
            return next.run(request).await;
        }
    };

    let remaining = limit.saturating_sub(rate_limit_state.current);

    if rate_limit_state.current > limit {
        record_rate_limit_rejected(WRITE_SCOPE);
        return AppError::rate_limited(
            "RATE_LIMITED",
            "rate limit exceeded",
            limit,
            0,
            reset_epoch_seconds,
            retry_after_seconds,
        );
    }

    record_rate_limit_allowed(WRITE_SCOPE);
    request.extensions_mut().insert(authenticated_user);
    let mut response = next.run(request).await;
    set_rate_limit_headers(
        &mut response,
        limit,
        remaining,
        reset_epoch_seconds,
        None,
    );
    response
}

pub async fn require_read_rate_limit(
    axum::extract::State(state): axum::extract::State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    let now_seconds = current_unix_timestamp();
    let limit = state.read_rate_limit_per_minute;
    let client_ip = client_ip(&request);
    let (key, reset_epoch_seconds, retry_after_seconds) =
        rate_limit_window_key("read", &client_ip, now_seconds);

    let rate_limit_state = match increment_and_read_rate_limit(&state, &key, retry_after_seconds).await {
        Ok(state) => state,
        Err(error) => {
            record_rate_limit_fail_open(READ_SCOPE);
            warn!(error = %error, "redis unavailable for read rate limiting, allowing request");
            return next.run(request).await;
        }
    };

    let remaining = limit.saturating_sub(rate_limit_state.current);

    if rate_limit_state.current > limit {
        record_rate_limit_rejected(READ_SCOPE);
        return AppError::rate_limited(
            "RATE_LIMITED",
            "rate limit exceeded",
            limit,
            0,
            reset_epoch_seconds,
            retry_after_seconds,
        );
    }

    record_rate_limit_allowed(READ_SCOPE);
    let mut response = next.run(request).await;
    set_rate_limit_headers(
        &mut response,
        limit,
        remaining,
        reset_epoch_seconds,
        None,
    );
    response
}

async fn increment_and_read_rate_limit(
    state: &AppState,
    key: &str,
    ttl_seconds: u64,
) -> Result<RateLimitState, redis::RedisError> {
    let mut connection = state.redis_client.get_multiplexed_async_connection().await?;
    let current: u32 = connection.incr(key, 1).await?;

    if current == 1 {
        let _: bool = connection.expire(key, ttl_seconds as i64).await?;
    }

    Ok(RateLimitState { current })
}

fn current_unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time must be after unix epoch")
        .as_secs()
}

fn rate_limit_window_key(scope: &str, subject: &str, now_seconds: u64) -> (String, u64, u64) {
    let window_seconds = 60_u64;
    let window_start = now_seconds / window_seconds;
    let reset_epoch_seconds = (window_start + 1) * window_seconds;
    let retry_after_seconds = reset_epoch_seconds.saturating_sub(now_seconds);
    let key = format!("rate_limit:{scope}:{subject}:{window_start}");

    (key, reset_epoch_seconds, retry_after_seconds)
}

fn client_ip(request: &Request) -> String {
    if let Some(value) = request.headers().get("x-forwarded-for")
        && let Ok(value) = value.to_str()
        && let Some(ip) = value.split(',').next()
    {
        let ip = ip.trim();

        if !ip.is_empty() {
            return ip.to_string();
        }
    }

    if let Some(value) = request.headers().get("x-real-ip")
        && let Ok(value) = value.to_str()
    {
        let ip = value.trim();

        if !ip.is_empty() {
            return ip.to_string();
        }
    }

    if let Some(ConnectInfo(addr)) = request.extensions().get::<ConnectInfo<SocketAddr>>() {
        return addr.ip().to_string();
    }

    "unknown".to_string()
}

struct RateLimitState {
    current: u32,
}
