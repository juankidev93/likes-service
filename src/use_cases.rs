#![allow(dead_code)]

use crate::domain::{ContentId, ContentType, UserId};
use crate::error::AppError;
use crate::likes_repository::{
    DeleteLikeResult, InsertLikeResult, PostgresLikesRepository,
};
use redis::{AsyncCommands, Client as RedisClient};

const LIKE_COUNT_CACHE_TTL_SECONDS: u64 = 60;

pub struct LikesUseCases<'a> {
    repository: PostgresLikesRepository<'a>,
    redis_client: RedisClient,
}

impl<'a> LikesUseCases<'a> {
    pub fn new(repository: PostgresLikesRepository<'a>, redis_client: RedisClient) -> Self {
        Self {
            repository,
            redis_client,
        }
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
            InsertLikeResult::Inserted => {
                self.increment_cached_like_count(content_type, content_id).await?;
                LikeContentResult::Liked
            }
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
            DeleteLikeResult::Deleted => {
                self.decrement_cached_like_count(content_type, content_id)
                    .await?;
                UnlikeContentResult::Unliked
            }
            DeleteLikeResult::NotFound => UnlikeContentResult::NotLiked,
        })
    }

    async fn increment_cached_like_count(
        &self,
        content_type: &ContentType,
        content_id: &ContentId,
    ) -> Result<(), AppError> {
        let key = like_count_cache_key(content_type, content_id);
        let mut redis_connection = self.redis_client.get_multiplexed_async_connection().await?;
        let cache_exists: bool = redis_connection.exists(&key).await?;

        if cache_exists {
            let _: i64 = redis_connection.incr(&key, 1).await?;
            let _: bool = redis_connection.expire(&key, LIKE_COUNT_CACHE_TTL_SECONDS as i64).await?;
        }

        Ok(())
    }

    async fn decrement_cached_like_count(
        &self,
        content_type: &ContentType,
        content_id: &ContentId,
    ) -> Result<(), AppError> {
        let key = like_count_cache_key(content_type, content_id);
        let mut redis_connection = self.redis_client.get_multiplexed_async_connection().await?;
        let cache_exists: bool = redis_connection.exists(&key).await?;

        if cache_exists {
            let new_count: i64 = redis_connection.decr(&key, 1).await?;

            if new_count < 0 {
                let _: () = redis_connection.set(&key, 0).await?;
            }

            let _: bool = redis_connection.expire(&key, LIKE_COUNT_CACHE_TTL_SECONDS as i64).await?;
        }

        Ok(())
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

fn like_count_cache_key(content_type: &ContentType, content_id: &ContentId) -> String {
    format!("likes:count:{content_type}:{content_id}")
}
