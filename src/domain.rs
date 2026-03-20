#![allow(dead_code)]

use crate::error::DomainError;
use std::{fmt, str::FromStr, time::SystemTime};
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ContentType(String);

impl ContentType {
    pub fn new(value: impl Into<String>) -> Result<Self, DomainError> {
        Self::from_str(&value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ContentType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for ContentType {
    type Err = DomainError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let normalized = value.trim().to_ascii_lowercase();

        if normalized.is_empty() {
            return Err(DomainError::InvalidContentType(value.to_string()));
        }

        let mut chars = normalized.chars();
        let Some(first) = chars.next() else {
            return Err(DomainError::InvalidContentType(value.to_string()));
        };

        if !first.is_ascii_lowercase() {
            return Err(DomainError::InvalidContentType(value.to_string()));
        }

        if !chars.all(|character| character.is_ascii_lowercase() || character.is_ascii_digit() || character == '_') {
            return Err(DomainError::InvalidContentType(value.to_string()));
        }

        Ok(Self(normalized))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ContentId(Uuid);

impl ContentId {
    pub fn new(value: Uuid) -> Self {
        Self(value)
    }

    pub fn as_uuid(&self) -> Uuid {
        self.0
    }
}

impl fmt::Display for ContentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for ContentId {
    type Err = DomainError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Uuid::parse_str(value)
            .map(Self)
            .map_err(|_| DomainError::InvalidContentId(value.to_string()))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct UserId(Uuid);

impl UserId {
    pub fn new(value: Uuid) -> Self {
        Self(value)
    }

    pub fn as_uuid(&self) -> Uuid {
        self.0
    }

    pub fn as_external_id(&self) -> String {
        format!("usr_{}", self.0)
    }
}

impl fmt::Display for UserId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for UserId {
    type Err = DomainError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let normalized = value.trim().strip_prefix("usr_").unwrap_or(value.trim());

        Uuid::parse_str(normalized)
            .map(Self)
            .map_err(|_| DomainError::InvalidUserId(value.to_string()))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Like {
    pub user_id: UserId,
    pub content_type: ContentType,
    pub content_id: ContentId,
    pub liked_at: SystemTime,
}

impl Like {
    pub fn new(
        user_id: UserId,
        content_type: ContentType,
        content_id: ContentId,
        liked_at: SystemTime,
    ) -> Self {
        Self {
            user_id,
            content_type,
            content_id,
            liked_at,
        }
    }
}
