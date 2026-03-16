#![allow(dead_code)]

use crate::content_registry::ContentTypeRegistry;
use reqwest::{Client, StatusCode};
use std::{error::Error, fmt};

#[derive(Clone)]
pub struct ContentValidationClient {
    registry: ContentTypeRegistry,
    http_client: Client,
}

impl ContentValidationClient {
    pub fn new(registry: ContentTypeRegistry) -> Self {
        Self {
            registry,
            http_client: Client::new(),
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

        let url = format!(
            "{}/v1/{}/{}",
            definition.base_url.trim_end_matches('/'),
            content_type,
            content_id
        );

        let response = self
            .http_client
            .get(url)
            .send()
            .await
            .map_err(|error| ContentValidationError::NetworkError(error.to_string()))?;

        match response.status() {
            StatusCode::OK => Ok(()),
            StatusCode::NOT_FOUND => Err(ContentValidationError::ContentNotFound {
                content_type: content_type.to_string(),
                content_id: content_id.to_string(),
            }),
            status => Err(ContentValidationError::DependencyUnavailable(format!(
                "unexpected content api status: {status}"
            ))),
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
