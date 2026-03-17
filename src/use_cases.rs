#![allow(dead_code)]

use crate::domain::{ContentId, ContentType, UserId};
use crate::error::AppError;
use crate::infra::metrics::record_like_operation;
use crate::integrations::content_validation::ContentValidationClient;
use crate::storage::likes_repository::{
    DeleteLikeResult, InsertLikeResult, PostgresLikesRepository,
};
use redis::{AsyncCommands, Client as RedisClient};

pub struct LikesUseCases<'a> {
    repository: PostgresLikesRepository<'a>,
    redis_client: RedisClient,
    content_validation_client: ContentValidationClient,
    cache_ttl_like_counts_seconds: u64,
}

impl<'a> LikesUseCases<'a> {
    pub fn new(
        repository: PostgresLikesRepository<'a>,
        redis_client: RedisClient,
        content_validation_client: ContentValidationClient,
        cache_ttl_like_counts_seconds: u64,
    ) -> Self {
        Self {
            repository,
            redis_client,
            content_validation_client,
            cache_ttl_like_counts_seconds,
        }
    }

    pub async fn like_content(
        &self,
        user_id: &UserId,
        content_type: &ContentType,
        content_id: &ContentId,
    ) -> Result<LikeContentResult, AppError> {
        self.validate_content(content_type, content_id).await?;

        let result = self
            .repository
            .insert_like(user_id, content_type, content_id)
            .await?;

        let already_existed = match result {
            InsertLikeResult::Inserted => {
                self.increment_cached_like_count(content_type, content_id).await?;
                record_like_operation(content_type.as_str(), "like");
                false
            }
            InsertLikeResult::AlreadyExists => true,
        };

        let like_status = self
            .repository
            .get_like_status(user_id, content_type, content_id)
            .await?;
        let like_count = self.repository.get_like_count(content_type, content_id).await?;

        Ok(LikeContentResult {
            liked: true,
            already_existed,
            count: like_count.count,
            liked_at: like_status.liked_at,
        })
    }

    pub async fn unlike_content(
        &self,
        user_id: &UserId,
        content_type: &ContentType,
        content_id: &ContentId,
    ) -> Result<UnlikeContentResult, AppError> {
        self.validate_content(content_type, content_id).await?;

        let result = self
            .repository
            .delete_like(user_id, content_type, content_id)
            .await?;

        let was_liked = match result {
            DeleteLikeResult::Deleted => {
                self.decrement_cached_like_count(content_type, content_id)
                    .await?;
                record_like_operation(content_type.as_str(), "unlike");
                true
            }
            DeleteLikeResult::NotFound => false,
        };

        let like_count = self.repository.get_like_count(content_type, content_id).await?;

        Ok(UnlikeContentResult {
            liked: false,
            was_liked,
            count: like_count.count,
        })
    }

    async fn validate_content(
        &self,
        content_type: &ContentType,
        content_id: &ContentId,
    ) -> Result<(), AppError> {
        self.content_validation_client
            .validate_content(content_type.as_str(), &content_id.to_string())
            .await?;

        Ok(())
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
            let _: bool = redis_connection
                .expire(&key, self.cache_ttl_like_counts_seconds as i64)
                .await?;
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

            let _: bool = redis_connection
                .expire(&key, self.cache_ttl_like_counts_seconds as i64)
                .await?;
        }

        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LikeContentResult {
    pub liked: bool,
    pub already_existed: bool,
    pub count: i64,
    pub liked_at: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnlikeContentResult {
    pub liked: bool,
    pub was_liked: bool,
    pub count: i64,
}

fn like_count_cache_key(content_type: &ContentType, content_id: &ContentId) -> String {
    format!("likes:count:{content_type}:{content_id}")
}
