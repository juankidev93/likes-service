use crate::app_state::AppState;
use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use redis::AsyncCommands;
use serde::Serialize;

pub async fn ready_health(State(state): State<AppState>) -> Response {
    let postgres = check_postgres(&state).await;
    let redis = check_redis(&state).await;
    let content_api = check_content_api(&state).await;

    let response = ReadinessResponse {
        status: if postgres.ok && redis.ok && content_api.ok {
            "ready"
        } else {
            "not_ready"
        },
        checks: ReadinessChecks {
            postgres: postgres.status,
            redis: redis.status,
            content_api: content_api.status,
        },
    };

    let status = if response.status == "ready" {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };

    (status, Json(response)).into_response()
}

async fn check_postgres(state: &AppState) -> DependencyCheckResult {
    let write_ok = sqlx::query_scalar::<_, i32>("SELECT 1")
        .fetch_one(&state.db_pool)
        .await
        .is_ok();
    let read_ok = sqlx::query_scalar::<_, i32>("SELECT 1")
        .fetch_one(&state.read_db_pool)
        .await
        .is_ok();

    if write_ok && read_ok {
        DependencyCheckResult::ok()
    } else {
        DependencyCheckResult::failed()
    }
}

async fn check_redis(state: &AppState) -> DependencyCheckResult {
    let mut connection = match state.redis_client.get_multiplexed_async_connection().await {
        Ok(connection) => connection,
        Err(_) => return DependencyCheckResult::failed(),
    };

    match connection.ping::<String>().await {
        Ok(_) => DependencyCheckResult::ok(),
        Err(_) => DependencyCheckResult::failed(),
    }
}

async fn check_content_api(state: &AppState) -> DependencyCheckResult {
    match state.content_validation_client.check_availability().await {
        Ok(_) => DependencyCheckResult::ok(),
        Err(_) => DependencyCheckResult::failed(),
    }
}

struct DependencyCheckResult {
    ok: bool,
    status: &'static str,
}

impl DependencyCheckResult {
    fn ok() -> Self {
        Self {
            ok: true,
            status: "ok",
        }
    }

    fn failed() -> Self {
        Self {
            ok: false,
            status: "failed",
        }
    }
}

#[derive(Serialize)]
struct ReadinessResponse {
    status: &'static str,
    checks: ReadinessChecks,
}

#[derive(Serialize)]
struct ReadinessChecks {
    postgres: &'static str,
    redis: &'static str,
    content_api: &'static str,
}
