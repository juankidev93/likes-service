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
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Domain(error) => write!(f, "{error}"),
        }
    }
}

impl Error for AppError {}

impl From<DomainError> for AppError {
    fn from(value: DomainError) -> Self {
        Self::Domain(value)
    }
}
