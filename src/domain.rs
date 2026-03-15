#![allow(dead_code)]

use crate::error::DomainError;
use std::{fmt, str::FromStr, time::SystemTime};
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ContentType {
    Post,
    Comment,
}

impl ContentType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Post => "post",
            Self::Comment => "comment",
        }
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
        match value.trim().to_ascii_lowercase().as_str() {
            "post" => Ok(Self::Post),
            "comment" => Ok(Self::Comment),
            _ => Err(DomainError::InvalidContentType(value.to_string())),
        }
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
}

impl fmt::Display for UserId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for UserId {
    type Err = DomainError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Uuid::parse_str(value)
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
