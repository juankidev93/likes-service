#![allow(dead_code)]

use crate::circuit_breaker::CircuitBreaker;
use crate::content_registry::ContentTypeRegistry;
use crate::metrics::record_external_call;
use reqwest::{Client, StatusCode};
use std::{error::Error, fmt, time::Instant};

#[derive(Clone)]
pub struct ContentValidationClient {
    registry: ContentTypeRegistry,
    http_client: Client,
    circuit_breaker: CircuitBreaker,
}

impl ContentValidationClient {
    pub fn new(registry: ContentTypeRegistry, circuit_breaker: CircuitBreaker) -> Self {
        Self {
            registry,
            http_client: Client::new(),
            circuit_breaker,
        }
    }

    pub async fn validate_content(
        &self,
        content_type: &str,
        content_id: &str,
    ) -> Result<(), ContentValidationError> {
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
        let response = self
            .http_client
            .get(url)
            .send()
            .await;

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
                Ok(())
            }
            StatusCode::NOT_FOUND => {
                self.circuit_breaker.record_success();
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
