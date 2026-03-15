#![allow(dead_code)]

use crate::domain::{ContentId, ContentType, UserId};
use crate::error::AppError;
use crate::likes_repository::{
    DeleteLikeResult, InsertLikeResult, PostgresLikesRepository,
};

pub struct LikesUseCases<'a> {
    repository: PostgresLikesRepository<'a>,
}

impl<'a> LikesUseCases<'a> {
    pub fn new(repository: PostgresLikesRepository<'a>) -> Self {
        Self { repository }
    }

    pub async fn like_content(
        &self,
        user_id: &UserId,
        content_type: &ContentType,
        content_id: &ContentId,
    ) -> Result<LikeContentResult, AppError> {
        let result = self
            .repository
            .insert_like(user_id, content_type, content_id)
            .await?;

        Ok(match result {
            InsertLikeResult::Inserted => LikeContentResult::Liked,
            InsertLikeResult::AlreadyExists => LikeContentResult::AlreadyLiked,
        })
    }

    pub async fn unlike_content(
        &self,
        user_id: &UserId,
        content_type: &ContentType,
        content_id: &ContentId,
    ) -> Result<UnlikeContentResult, AppError> {
        let result = self
            .repository
            .delete_like(user_id, content_type, content_id)
            .await?;

        Ok(match result {
            DeleteLikeResult::Deleted => UnlikeContentResult::Unliked,
            DeleteLikeResult::NotFound => UnlikeContentResult::NotLiked,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LikeContentResult {
    Liked,
    AlreadyLiked,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnlikeContentResult {
    Unliked,
    NotLiked,
}
