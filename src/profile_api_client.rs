#![allow(dead_code)]

use reqwest::{header, Client, StatusCode};
use serde::Deserialize;
use std::{error::Error, fmt};

#[derive(Clone)]
pub struct ProfileApiClient {
    base_url: String,
    http_client: Client,
}

impl ProfileApiClient {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            http_client: Client::new(),
        }
    }

    pub async fn validate_token(
        &self,
        bearer_token: &str,
    ) -> Result<AuthenticatedUser, AuthError> {
        let response = self
            .http_client
            .get(format!("{}/v1/auth/validate", self.base_url))
            .header(header::AUTHORIZATION, format!("Bearer {bearer_token}"))
            .send()
            .await
            .map_err(|error| AuthError::NetworkError(error.to_string()))?;

        match response.status() {
            StatusCode::OK => response
                .json::<ValidateTokenResponse>()
                .await
                .map(Into::into)
                .map_err(|error| AuthError::DependencyUnavailable(error.to_string())),
            StatusCode::UNAUTHORIZED => Err(AuthError::InvalidToken),
            status => Err(AuthError::DependencyUnavailable(format!(
                "unexpected profile api status: {status}"
            ))),
        }
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
