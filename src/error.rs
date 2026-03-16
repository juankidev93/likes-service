#![allow(dead_code)]

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
    Domain(DomainError),
    Database(sqlx::Error),
    Cache(redis::RedisError),
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Domain(error) => write!(f, "{error}"),
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
