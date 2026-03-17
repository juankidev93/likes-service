#![allow(dead_code)]

use crate::infra::metrics::{record_cache_operation, record_external_call};
use crate::integrations::content_registry::ContentTypeRegistry;
use crate::resilience::circuit_breaker::CircuitBreaker;
use redis::AsyncCommands;
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};
use std::{error::Error, fmt, time::Instant};

#[derive(Clone)]
pub struct ContentValidationClient {
    registry: ContentTypeRegistry,
    http_client: Client,
    redis_client: redis::Client,
    cache_ttl_seconds: u64,
    circuit_breaker: CircuitBreaker,
}

impl ContentValidationClient {
    pub fn new(
        registry: ContentTypeRegistry,
        redis_client: redis::Client,
        cache_ttl_seconds: u64,
        circuit_breaker: CircuitBreaker,
    ) -> Self {
        Self {
            registry,
            http_client: Client::new(),
            redis_client,
            cache_ttl_seconds,
            circuit_breaker,
        }
    }

    pub async fn validate_content(
        &self,
        content_type: &str,
        content_id: &str,
    ) -> Result<(), ContentValidationError> {
        if let Some(cached_result) = self
            .get_cached_validation_result(content_type, content_id)
            .await
        {
            return cached_result.into_result(content_type, content_id);
        }

        let definition = self
            .registry
            .get(content_type)
            .ok_or_else(|| ContentValidationError::ContentTypeUnknown(content_type.to_string()))?;

        if let Err(error) = self.circuit_breaker.allow_request() {
            record_external_call(
                "content_api",
                "GET /v1/{content_type}/{content_id}",
                "circuit_open",
                0.0,
            );
            return Err(ContentValidationError::DependencyUnavailable(error.to_string()));
        }

        let url = format!(
            "{}/v1/{}/{}",
            definition.base_url.trim_end_matches('/'),
            content_type,
            content_id
        );

        let start = Instant::now();
        let response = self.http_client.get(url).send().await;

        let response = match response {
            Ok(response) => response,
            Err(error) => {
                self.circuit_breaker.record_failure();
                record_external_call(
                    "content_api",
                    "GET /v1/{content_type}/{content_id}",
                    "network_error",
                    start.elapsed().as_secs_f64(),
                );
                return Err(ContentValidationError::NetworkError(error.to_string()));
            }
        };

        let status = response.status();
        record_external_call(
            "content_api",
            "GET /v1/{content_type}/{content_id}",
            status.as_str(),
            start.elapsed().as_secs_f64(),
        );

        match status {
            StatusCode::OK => {
                self.circuit_breaker.record_success();
                self.cache_validation_result(
                    content_type,
                    content_id,
                    CachedValidationResult::Exists,
                )
                .await;
                Ok(())
            }
            StatusCode::NOT_FOUND => {
                self.circuit_breaker.record_success();
                self.cache_validation_result(
                    content_type,
                    content_id,
                    CachedValidationResult::NotFound,
                )
                .await;
                Err(ContentValidationError::ContentNotFound {
                    content_type: content_type.to_string(),
                    content_id: content_id.to_string(),
                })
            }
            status => {
                self.circuit_breaker.record_failure();
                Err(ContentValidationError::DependencyUnavailable(format!(
                    "unexpected content api status: {status}"
                )))
            }
        }
    }

    pub async fn check_availability(&self) -> Result<(), ContentValidationError> {
        let definitions = self.registry.all();

        if definitions.is_empty() {
            return Err(ContentValidationError::DependencyUnavailable(
                "no content api definitions configured".to_string(),
            ));
        }

        let mut last_error = None;

        for definition in definitions {
            if let Err(error) = self.circuit_breaker.allow_request() {
                record_external_call(
                    "content_api",
                    "GET /v1/{content_type}/{content_id}",
                    "circuit_open",
                    0.0,
                );
                last_error = Some(ContentValidationError::DependencyUnavailable(error.to_string()));
                continue;
            }

            let url = format!(
                "{}/v1/{}/{}",
                definition.base_url.trim_end_matches('/'),
                definition.content_type,
                "00000000-0000-0000-0000-000000000000"
            );

            let start = Instant::now();
            let response = self.http_client.get(url).send().await;

            let response = match response {
                Ok(response) => response,
                Err(error) => {
                    self.circuit_breaker.record_failure();
                    record_external_call(
                        "content_api",
                        "GET /v1/{content_type}/{content_id}",
                        "network_error",
                        start.elapsed().as_secs_f64(),
                    );
                    last_error = Some(ContentValidationError::NetworkError(error.to_string()));
                    continue;
                }
            };

            let status = response.status();
            record_external_call(
                "content_api",
                "GET /v1/{content_type}/{content_id}",
                status.as_str(),
                start.elapsed().as_secs_f64(),
            );

            match status {
                StatusCode::OK | StatusCode::NOT_FOUND => {
                    self.circuit_breaker.record_success();
                    return Ok(());
                }
                status => {
                    self.circuit_breaker.record_failure();
                    last_error = Some(ContentValidationError::DependencyUnavailable(format!(
                        "unexpected content api status: {status}"
                    )));
                }
            }
        }

        Err(last_error.unwrap_or_else(|| {
            ContentValidationError::DependencyUnavailable(
                "content api availability check failed".to_string(),
            )
        }))
    }

    async fn get_cached_validation_result(
        &self,
        content_type: &str,
        content_id: &str,
    ) -> Option<CachedValidationResult> {
        let key = content_validation_cache_key(content_type, content_id);
        let mut redis_connection = match self.redis_client.get_multiplexed_async_connection().await {
            Ok(connection) => connection,
            Err(error) => {
                record_cache_operation("validate_content", "error");
                tracing::warn!(service = "likes_service", error = %error, "redis unavailable for content validation cache read");
                return None;
            }
        };

        let cached_value: Option<String> = match redis_connection.get(&key).await {
            Ok(value) => value,
            Err(error) => {
                record_cache_operation("validate_content", "error");
                tracing::warn!(service = "likes_service", error = %error, "failed to read content validation cache entry");
                return None;
            }
        };

        match cached_value {
            Some(payload) => match serde_json::from_str::<CachedValidationResult>(&payload) {
                Ok(result) => {
                    record_cache_operation("validate_content", "hit");
                    Some(result)
                }
                Err(error) => {
                    record_cache_operation("validate_content", "error");
                    tracing::warn!(service = "likes_service", error = %error, "failed to decode content validation cache entry");
                    None
                }
            },
            None => {
                record_cache_operation("validate_content", "miss");
                None
            }
        }
    }

    async fn cache_validation_result(
        &self,
        content_type: &str,
        content_id: &str,
        result: CachedValidationResult,
    ) {
        let key = content_validation_cache_key(content_type, content_id);
        let payload = match serde_json::to_string(&result) {
            Ok(payload) => payload,
            Err(error) => {
                tracing::warn!(service = "likes_service", error = %error, "failed to serialize content validation cache entry");
                return;
            }
        };

        let mut redis_connection = match self.redis_client.get_multiplexed_async_connection().await {
            Ok(connection) => connection,
            Err(error) => {
                record_cache_operation("validate_content", "error");
                tracing::warn!(service = "likes_service", error = %error, "redis unavailable for content validation cache write");
                return;
            }
        };

        if let Err(error) = redis_connection
            .set_ex::<_, _, ()>(&key, payload, self.cache_ttl_seconds)
            .await
        {
            record_cache_operation("validate_content", "error");
            tracing::warn!(service = "likes_service", error = %error, "failed to write content validation cache entry");
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
enum CachedValidationResult {
    Exists,
    NotFound,
}

impl CachedValidationResult {
    fn into_result(
        self,
        content_type: &str,
        content_id: &str,
    ) -> Result<(), ContentValidationError> {
        match self {
            Self::Exists => Ok(()),
            Self::NotFound => Err(ContentValidationError::ContentNotFound {
                content_type: content_type.to_string(),
                content_id: content_id.to_string(),
            }),
        }
    }
}

fn content_validation_cache_key(content_type: &str, content_id: &str) -> String {
    format!("content_validation:{content_type}:{content_id}")
}

#[derive(Debug, PartialEq, Eq)]
pub enum ContentValidationError {
    ContentTypeUnknown(String),
    ContentNotFound {
        content_type: String,
        content_id: String,
    },
    DependencyUnavailable(String),
    NetworkError(String),
}

impl fmt::Display for ContentValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ContentTypeUnknown(content_type) => {
                write!(f, "unknown content type: {content_type}")
            }
            Self::ContentNotFound {
                content_type,
                content_id,
            } => write!(f, "content not found: {content_type}/{content_id}"),
            Self::DependencyUnavailable(message) => {
                write!(f, "content api dependency unavailable: {message}")
            }
            Self::NetworkError(message) => write!(f, "content api network error: {message}"),
        }
    }
}

impl Error for ContentValidationError {}
