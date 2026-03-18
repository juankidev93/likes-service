#![allow(dead_code)]

use crate::infra::metrics::record_external_call;
use crate::resilience::circuit_breaker::CircuitBreaker;
use reqwest::{header, Client, StatusCode};
use serde::Deserialize;
use std::{error::Error, fmt, time::Instant};
use tracing::{info, warn};

#[derive(Clone)]
pub struct ProfileApiClient {
    base_url: String,
    http_client: Client,
    circuit_breaker: CircuitBreaker,
}

impl ProfileApiClient {
    pub fn new(base_url: impl Into<String>, circuit_breaker: CircuitBreaker) -> Self {
        Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            http_client: Client::new(),
            circuit_breaker,
        }
    }

    pub async fn validate_token(
        &self,
        bearer_token: &str,
    ) -> Result<AuthenticatedUser, AuthError> {
        if let Err(error) = self.circuit_breaker.allow_request() {
            record_external_call("profile_api", "GET /v1/auth/validate", "circuit_open", 0.0);
            warn!(
                service = "likes_service",
                dependency = "profile_api",
                method = "GET /v1/auth/validate",
                latency_ms = 0,
                success = false,
                status = "circuit_open",
                "external call blocked by circuit breaker"
            );
            return Err(AuthError::DependencyUnavailable(error.to_string()));
        }

        let start = Instant::now();
        let response = self
            .http_client
            .get(format!("{}/v1/auth/validate", self.base_url))
            .header(header::AUTHORIZATION, format!("Bearer {bearer_token}"))
            .send()
            .await;

        let response = match response {
            Ok(response) => response,
            Err(error) => {
                self.circuit_breaker.record_failure();
                log_external_call(
                    "profile_api",
                    "GET /v1/auth/validate",
                    start.elapsed().as_secs_f64(),
                    false,
                    "network_error",
                );
                record_external_call(
                    "profile_api",
                    "GET /v1/auth/validate",
                    "network_error",
                    start.elapsed().as_secs_f64(),
                );
                return Err(AuthError::NetworkError(error.to_string()));
            }
        };

        let status = response.status();
        let latency_seconds = start.elapsed().as_secs_f64();
        let success = status.is_success() || status == StatusCode::UNAUTHORIZED;
        log_external_call(
            "profile_api",
            "GET /v1/auth/validate",
            latency_seconds,
            success,
            status.as_str(),
        );
        record_external_call(
            "profile_api",
            "GET /v1/auth/validate",
            status.as_str(),
            latency_seconds,
        );

        match status {
            StatusCode::OK => match response.json::<ValidateTokenResponse>().await {
                Ok(payload) => {
                    match payload.into_authenticated_user() {
                        Ok(user) => {
                            self.circuit_breaker.record_success();
                            Ok(user)
                        }
                        Err(error) => {
                            self.circuit_breaker.record_success();
                            Err(error)
                        }
                    }
                }
                Err(error) => {
                    self.circuit_breaker.record_failure();
                    Err(AuthError::DependencyUnavailable(error.to_string()))
                }
            },
            StatusCode::UNAUTHORIZED => {
                self.circuit_breaker.record_success();
                Err(AuthError::InvalidToken)
            }
            status => {
                self.circuit_breaker.record_failure();
                Err(AuthError::DependencyUnavailable(format!(
                    "unexpected profile api status: {status}"
                )))
            }
        }
    }

    pub async fn check_availability(&self) -> Result<(), AuthError> {
        if let Err(error) = self.circuit_breaker.allow_request() {
            record_external_call("profile_api", "GET /v1/auth/validate", "circuit_open", 0.0);
            warn!(
                service = "likes_service",
                dependency = "profile_api",
                method = "GET /v1/auth/validate",
                latency_ms = 0,
                success = false,
                status = "circuit_open",
                "external call blocked by circuit breaker"
            );
            return Err(AuthError::DependencyUnavailable(error.to_string()));
        }

        let start = Instant::now();
        let response = self
            .http_client
            .get(format!("{}/v1/auth/validate", self.base_url))
            .header(header::AUTHORIZATION, "Bearer readiness-check-token")
            .send()
            .await;

        let response = match response {
            Ok(response) => response,
            Err(error) => {
                self.circuit_breaker.record_failure();
                log_external_call(
                    "profile_api",
                    "GET /v1/auth/validate",
                    start.elapsed().as_secs_f64(),
                    false,
                    "network_error",
                );
                record_external_call(
                    "profile_api",
                    "GET /v1/auth/validate",
                    "network_error",
                    start.elapsed().as_secs_f64(),
                );
                return Err(AuthError::NetworkError(error.to_string()));
            }
        };

        let status = response.status();
        let latency_seconds = start.elapsed().as_secs_f64();
        let success = status.is_success() || status == StatusCode::UNAUTHORIZED;
        log_external_call(
            "profile_api",
            "GET /v1/auth/validate",
            latency_seconds,
            success,
            status.as_str(),
        );
        record_external_call(
            "profile_api",
            "GET /v1/auth/validate",
            status.as_str(),
            latency_seconds,
        );

        match status {
            StatusCode::OK | StatusCode::UNAUTHORIZED => {
                self.circuit_breaker.record_success();
                Ok(())
            }
            status => {
                self.circuit_breaker.record_failure();
                Err(AuthError::DependencyUnavailable(format!(
                    "unexpected profile api status: {status}"
                )))
            }
        }
    }
}

fn log_external_call(
    dependency: &'static str,
    method: &'static str,
    latency_seconds: f64,
    success: bool,
    status: &str,
) {
    let latency_ms = (latency_seconds * 1000.0).round() as u64;

    if success {
        info!(
            service = "likes_service",
            dependency = dependency,
            method = method,
            latency_ms = latency_ms,
            success = success,
            status = status,
            "external call completed"
        );
    } else {
        warn!(
            service = "likes_service",
            dependency = dependency,
            method = method,
            latency_ms = latency_ms,
            success = success,
            status = status,
            "external call failed"
        );
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthenticatedUser {
    pub user_id: String,
    pub display_name: String,
}

#[derive(Debug)]
pub enum AuthError {
    InvalidToken,
    DependencyUnavailable(String),
    NetworkError(String),
}

impl fmt::Display for AuthError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidToken => write!(f, "invalid token"),
            Self::DependencyUnavailable(message) => {
                write!(f, "profile api dependency unavailable: {message}")
            }
            Self::NetworkError(message) => write!(f, "profile api network error: {message}"),
        }
    }
}

impl Error for AuthError {}

#[derive(Deserialize)]
struct ValidateTokenResponse {
    valid: bool,
    user_id: String,
    display_name: String,
}

impl From<ValidateTokenResponse> for AuthenticatedUser {
    fn from(value: ValidateTokenResponse) -> Self {
        Self {
            user_id: value.user_id,
            display_name: value.display_name,
        }
    }
}

impl ValidateTokenResponse {
    fn into_authenticated_user(self) -> Result<AuthenticatedUser, AuthError> {
        if !self.valid {
            return Err(AuthError::InvalidToken);
        }

        Ok(self.into())
    }
}
