#![allow(dead_code)]

use crate::domain::{ContentId, ContentType, UserId};
use crate::error::AppError;
use sqlx::PgPool;

pub struct PostgresLikesRepository<'a> {
    db_pool: &'a PgPool,
}

impl<'a> PostgresLikesRepository<'a> {
    pub fn new(db_pool: &'a PgPool) -> Self {
        Self { db_pool }
    }

    pub async fn insert_like(
        &self,
        user_id: &UserId,
        content_type: &ContentType,
        content_id: &ContentId,
    ) -> Result<InsertLikeResult, AppError> {
        let result = sqlx::query(
            r#"
            INSERT INTO likes (user_id, content_type, content_id)
            VALUES ($1, $2, $3)
            ON CONFLICT (user_id, content_type, content_id) DO NOTHING
            "#,
        )
        .bind(user_id.to_string())
        .bind(content_type.to_string())
        .bind(content_id.to_string())
        .execute(self.db_pool)
        .await?;

        Ok(if result.rows_affected() == 1 {
            InsertLikeResult::Inserted
        } else {
            InsertLikeResult::AlreadyExists
        })
    }

    pub async fn delete_like(
        &self,
        user_id: &UserId,
        content_type: &ContentType,
        content_id: &ContentId,
    ) -> Result<DeleteLikeResult, AppError> {
        let result = sqlx::query(
            r#"
            DELETE FROM likes
            WHERE user_id = $1
              AND content_type = $2
              AND content_id = $3
            "#,
        )
        .bind(user_id.to_string())
        .bind(content_type.to_string())
        .bind(content_id.to_string())
        .execute(self.db_pool)
        .await?;

        Ok(if result.rows_affected() == 1 {
            DeleteLikeResult::Deleted
        } else {
            DeleteLikeResult::NotFound
        })
    }

    pub async fn get_like_status(
        &self,
        user_id: &UserId,
        content_type: &ContentType,
        content_id: &ContentId,
    ) -> Result<LikeStatus, AppError> {
        let exists = sqlx::query_scalar::<_, bool>(
            r#"
            SELECT EXISTS (
                SELECT 1
                FROM likes
                WHERE user_id = $1
                  AND content_type = $2
                  AND content_id = $3
            )
            "#,
        )
        .bind(user_id.to_string())
        .bind(content_type.to_string())
        .bind(content_id.to_string())
        .fetch_one(self.db_pool)
        .await?;

        Ok(LikeStatus { exists })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InsertLikeResult {
    Inserted,
    AlreadyExists,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeleteLikeResult {
    Deleted,
    NotFound,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LikeStatus {
    pub exists: bool,
}