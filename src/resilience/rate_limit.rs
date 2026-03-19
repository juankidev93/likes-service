use crate::app_state::{AppState, ReadRateLimitLease};
use crate::auth_middleware::authenticate_headers;
use crate::error::{AppError, set_rate_limit_headers};
use crate::infra::logging::LoggedUserId;
use crate::infra::metrics::{
    record_rate_limit_allowed, record_rate_limit_fail_open, record_rate_limit_rejected,
};
use axum::{
    extract::{ConnectInfo, Request},
    middleware::Next,
    response::Response,
};
use once_cell::sync::Lazy;
use std::net::SocketAddr;
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::warn;

const WRITE_SCOPE: &str = "write_user";
const READ_SCOPE: &str = "read_ip";
const READ_LEASE_SIZE: u32 = 50;
static RATE_LIMIT_INCREMENT_SCRIPT: Lazy<redis::Script> = Lazy::new(|| {
    redis::Script::new(
        r#"
        local current = redis.call('INCR', KEYS[1])
        if current == 1 then
            redis.call('EXPIRE', KEYS[1], ARGV[1])
        end
        return current
        "#,
    )
});
static RATE_LIMIT_RESERVE_SCRIPT: Lazy<redis::Script> = Lazy::new(|| {
    redis::Script::new(
        r#"
        local current = tonumber(redis.call('GET', KEYS[1]) or '0')
        local ttl = tonumber(ARGV[1])
        local requested = tonumber(ARGV[2])
        local limit = tonumber(ARGV[3])

        local remaining = limit - current
        if remaining <= 0 then
            return {current, 0}
        end

        local granted = math.min(requested, remaining)
        current = redis.call('INCRBY', KEYS[1], granted)

        if current == granted then
            redis.call('EXPIRE', KEYS[1], ttl)
        end

        return {current, granted}
        "#,
    )
});

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

    let rate_limit_state =
        match increment_and_read_rate_limit(&state, &key, retry_after_seconds).await {
            Ok(state) => state,
            Err(error) => {
                record_rate_limit_fail_open(WRITE_SCOPE);
                warn!(
                    service = "likes_service",
                    error = %error,
                    "redis unavailable for write rate limiting, allowing request"
                );
                request.extensions_mut().insert(authenticated_user.clone());
                let mut response = next.run(request).await;
                response
                    .extensions_mut()
                    .insert(LoggedUserId(authenticated_user.user_id));
                return response;
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
    request.extensions_mut().insert(authenticated_user.clone());
    let mut response = next.run(request).await;
    response
        .extensions_mut()
        .insert(LoggedUserId(authenticated_user.user_id));
    set_rate_limit_headers(&mut response, limit, remaining, reset_epoch_seconds, None);
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

    let rate_limit_state = match consume_or_reserve_read_lease(
        &state,
        &key,
        limit,
        reset_epoch_seconds,
        retry_after_seconds,
    )
    .await
    {
        Ok(state) => state,
        Err(error) => {
            record_rate_limit_fail_open(READ_SCOPE);
            warn!(
                service = "likes_service",
                error = %error,
                "redis unavailable for read rate limiting, allowing request"
            );
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
    set_rate_limit_headers(&mut response, limit, remaining, reset_epoch_seconds, None);
    response
}

async fn consume_or_reserve_read_lease(
    state: &AppState,
    key: &str,
    limit: u32,
    reset_epoch_seconds: u64,
    retry_after_seconds: u64,
) -> Result<RateLimitState, redis::RedisError> {
    if let Some(current) = try_consume_read_lease(state, key, reset_epoch_seconds).await {
        return Ok(RateLimitState { current });
    }

    let (current_after_reservation, granted) =
        reserve_read_lease(state, key, limit, retry_after_seconds).await?;

    if granted == 0 {
        return Ok(RateLimitState {
            current: current_after_reservation.saturating_add(1),
        });
    }

    let current_floor = current_after_reservation.saturating_sub(granted);
    {
        let mut leases = state.read_rate_limit_leases.lock().await;
        leases.insert(
            key.to_string(),
            ReadRateLimitLease {
                current_floor,
                remaining: granted.saturating_sub(1),
                reset_epoch_seconds,
            },
        );
    }

    Ok(RateLimitState {
        current: current_floor.saturating_add(1),
    })
}

async fn try_consume_read_lease(
    state: &AppState,
    key: &str,
    reset_epoch_seconds: u64,
) -> Option<u32> {
    let mut leases = state.read_rate_limit_leases.lock().await;
    let lease = leases.get_mut(key)?;

    if lease.reset_epoch_seconds != reset_epoch_seconds {
        leases.remove(key);
        return None;
    }

    if lease.remaining == 0 {
        leases.remove(key);
        return None;
    }

    let consumed = lease.current_floor.saturating_add(1);
    lease.current_floor = consumed;
    lease.remaining = lease.remaining.saturating_sub(1);

    if lease.remaining == 0 {
        leases.remove(key);
    }

    Some(consumed)
}

async fn reserve_read_lease(
    state: &AppState,
    key: &str,
    limit: u32,
    retry_after_seconds: u64,
) -> Result<(u32, u32), redis::RedisError> {
    let mut connection = state.get_redis_connection().await?;
    let response: (u32, u32) = RATE_LIMIT_RESERVE_SCRIPT
        .key(key)
        .arg(retry_after_seconds as i64)
        .arg(READ_LEASE_SIZE as i64)
        .arg(limit as i64)
        .invoke_async(&mut connection)
        .await?;

    Ok(response)
}

async fn increment_and_read_rate_limit(
    state: &AppState,
    key: &str,
    ttl_seconds: u64,
) -> Result<RateLimitState, redis::RedisError> {
    let mut connection = state.get_redis_connection().await?;
    let current: u32 = RATE_LIMIT_INCREMENT_SCRIPT
        .key(key)
        .arg(ttl_seconds as i64)
        .invoke_async(&mut connection)
        .await?;

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
