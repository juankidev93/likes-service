#![allow(dead_code)]

use crate::infra::logging::ErrorLogContext;
use crate::integrations::content_validation::ContentValidationError;
use axum::{
    http::{header::HeaderName, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::Serialize;
use serde_json::{json, Value};
use std::{error::Error, fmt};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DomainError {
    InvalidContentType(String),
    InvalidContentId(String),
    InvalidUserId(String),
}

impl fmt::Display for DomainError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidContentType(value) => {
                write!(f, "invalid content type: '{value}'")
            }
            Self::InvalidContentId(value) => {
                write!(f, "invalid content id: '{value}'")
            }
            Self::InvalidUserId(value) => {
                write!(f, "invalid user id: '{value}'")
            }
        }
    }
}

impl Error for DomainError {}

#[derive(Debug)]
pub enum AppError {
    InvalidRequest {
        code: &'static str,
        message: String,
    },
    Unauthorized {
        code: &'static str,
        message: String,
    },
    DependencyUnavailable {
        code: &'static str,
        message: String,
    },
    Domain(DomainError),
    ContentValidation(ContentValidationError),
    Database(sqlx::Error),
    Cache(redis::RedisError),
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRequest { message, .. } => write!(f, "{message}"),
            Self::Unauthorized { message, .. } => write!(f, "{message}"),
            Self::DependencyUnavailable { message, .. } => write!(f, "{message}"),
            Self::Domain(error) => write!(f, "{error}"),
            Self::ContentValidation(error) => write!(f, "{error}"),
            Self::Database(error) => write!(f, "database error: {error}"),
            Self::Cache(error) => write!(f, "cache error: {error}"),
        }
    }
}

impl Error for AppError {}

impl From<DomainError> for AppError {
    fn from(value: DomainError) -> Self {
        Self::Domain(value)
    }
}

impl From<ContentValidationError> for AppError {
    fn from(value: ContentValidationError) -> Self {
        Self::ContentValidation(value)
    }
}

impl From<sqlx::Error> for AppError {
    fn from(value: sqlx::Error) -> Self {
        Self::Database(value)
    }
}

impl From<redis::RedisError> for AppError {
    fn from(value: redis::RedisError) -> Self {
        Self::Cache(value)
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let error_context = self.error_log_context();
        let (status, code, message, details) = self.as_http_error();

        let mut response = (
            status,
            Json(HttpErrorBody {
                error: HttpErrorDetail {
                    code,
                    message,
                    request_id: None,
                    details,
                },
            }),
        )
            .into_response();

        if let Some(error_context) = error_context {
            response.extensions_mut().insert(error_context);
        }

        response
    }
}

impl AppError {
    pub fn invalid_request(code: &'static str, message: impl Into<String>) -> Self {
        Self::InvalidRequest {
            code,
            message: message.into(),
        }
    }

    pub fn unauthorized(code: &'static str, message: impl Into<String>) -> Self {
        Self::Unauthorized {
            code,
            message: message.into(),
        }
    }

    pub fn dependency_unavailable(
        code: &'static str,
        message: impl Into<String>,
    ) -> Self {
        Self::DependencyUnavailable {
            code,
            message: message.into(),
        }
    }

    pub fn rate_limited(
        code: &'static str,
        message: impl Into<String>,
        limit: u32,
        remaining: u32,
        reset_epoch_seconds: u64,
        retry_after_seconds: u64,
    ) -> Response {
        let mut response = (
            StatusCode::TOO_MANY_REQUESTS,
            Json(HttpErrorBody {
                error: HttpErrorDetail {
                    code,
                    message: message.into(),
                    request_id: None,
                    details: Some(json!({
                        "limit": limit,
                        "remaining": remaining,
                        "reset_epoch_seconds": reset_epoch_seconds,
                        "retry_after_seconds": retry_after_seconds,
                    })),
                },
            }),
        )
            .into_response();

        set_rate_limit_headers(
            &mut response,
            limit,
            remaining,
            reset_epoch_seconds,
            Some(retry_after_seconds),
        );

        response
    }

    fn as_http_error(self) -> (StatusCode, &'static str, String, Option<Value>) {
        match self {
            Self::InvalidRequest { code, message } => {
                (StatusCode::BAD_REQUEST, code, message, None)
            }
            Self::Unauthorized { code, message } => {
                (StatusCode::UNAUTHORIZED, code, message, None)
            }
            Self::DependencyUnavailable { code, message } => {
                (StatusCode::SERVICE_UNAVAILABLE, code, message, None)
            }
            Self::Domain(error) => match error {
                DomainError::InvalidContentType(_) => (
                    StatusCode::BAD_REQUEST,
                    "INVALID_CONTENT_TYPE",
                    "invalid content type".to_string(),
                    Some(json!({
                        "field": "content_type",
                    })),
                ),
                DomainError::InvalidContentId(_) => (
                    StatusCode::BAD_REQUEST,
                    "INVALID_CONTENT_ID",
                    "invalid content id".to_string(),
                    Some(json!({
                        "field": "content_id",
                    })),
                ),
                DomainError::InvalidUserId(_) => (
                    StatusCode::BAD_REQUEST,
                    "INVALID_USER_ID",
                    "invalid user id".to_string(),
                    Some(json!({
                        "field": "user_id",
                    })),
                ),
            },
            Self::ContentValidation(error) => match error {
                ContentValidationError::ContentTypeUnknown(content_type) => (
                    StatusCode::BAD_REQUEST,
                    "CONTENT_TYPE_UNKNOWN",
                    "content type unknown".to_string(),
                    Some(json!({
                        "content_type": content_type,
                    })),
                ),
                ContentValidationError::ContentNotFound {
                    content_type,
                    content_id,
                } => (
                    StatusCode::NOT_FOUND,
                    "CONTENT_NOT_FOUND",
                    "content not found".to_string(),
                    Some(json!({
                        "content_type": content_type,
                        "content_id": content_id,
                    })),
                ),
                ContentValidationError::DependencyUnavailable(_) => (
                    StatusCode::SERVICE_UNAVAILABLE,
                    "DEPENDENCY_UNAVAILABLE",
                    "dependency unavailable".to_string(),
                    None,
                ),
                ContentValidationError::NetworkError(_) => (
                    StatusCode::SERVICE_UNAVAILABLE,
                    "DEPENDENCY_UNAVAILABLE",
                    "dependency unavailable".to_string(),
                    None,
                ),
            },
            Self::Database(_) | Self::Cache(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "INTERNAL_ERROR",
                "internal error".to_string(),
                None,
            ),
        }
    }

    fn error_log_context(&self) -> Option<ErrorLogContext> {
        match self {
            Self::Database(error) => Some(ErrorLogContext {
                error_type: "database_error",
                error_message: error.to_string(),
                stack_trace: Some(std::backtrace::Backtrace::force_capture().to_string()),
            }),
            Self::Cache(error) => Some(ErrorLogContext {
                error_type: "cache_error",
                error_message: error.to_string(),
                stack_trace: Some(std::backtrace::Backtrace::force_capture().to_string()),
            }),
            _ => None,
        }
    }
}

pub fn set_rate_limit_headers(
    response: &mut Response,
    limit: u32,
    remaining: u32,
    reset_epoch_seconds: u64,
    retry_after_seconds: Option<u64>,
) {
    response.headers_mut().insert(
        HeaderName::from_static("x-ratelimit-limit"),
        HeaderValue::from_str(&limit.to_string()).expect("rate limit header must be valid"),
    );
    response.headers_mut().insert(
        HeaderName::from_static("x-ratelimit-remaining"),
        HeaderValue::from_str(&remaining.to_string())
            .expect("rate limit remaining header must be valid"),
    );
    response.headers_mut().insert(
        HeaderName::from_static("x-ratelimit-reset"),
        HeaderValue::from_str(&reset_epoch_seconds.to_string())
            .expect("rate limit reset header must be valid"),
    );

    if let Some(retry_after_seconds) = retry_after_seconds {
        response.headers_mut().insert(
            HeaderName::from_static("retry-after"),
            HeaderValue::from_str(&retry_after_seconds.to_string())
                .expect("retry-after header must be valid"),
        );
    }
}

#[derive(Serialize)]
struct HttpErrorBody {
    error: HttpErrorDetail,
}

#[derive(Serialize)]
struct HttpErrorDetail {
    code: &'static str,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    request_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    details: Option<Value>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn domain_errors_map_to_expected_http_errors() {
        let (status, code, message, details) =
            AppError::from(DomainError::InvalidContentType("unknown".to_string())).as_http_error();
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(code, "INVALID_CONTENT_TYPE");
        assert_eq!(message, "invalid content type");
        assert_eq!(details, Some(json!({ "field": "content_type" })));

        let (status, code, message, details) =
            AppError::from(DomainError::InvalidContentId("bad-id".to_string())).as_http_error();
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(code, "INVALID_CONTENT_ID");
        assert_eq!(message, "invalid content id");
        assert_eq!(details, Some(json!({ "field": "content_id" })));
    }

    #[test]
    fn content_validation_not_found_maps_to_404() {
        let error = AppError::from(ContentValidationError::ContentNotFound {
            content_type: "post".to_string(),
            content_id: "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaa1".to_string(),
        });

        let (status, code, message, details) = error.as_http_error();

        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(code, "CONTENT_NOT_FOUND");
        assert_eq!(message, "content not found");
        assert_eq!(
            details,
            Some(json!({
                "content_type": "post",
                "content_id": "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaa1",
            }))
        );
    }
}
