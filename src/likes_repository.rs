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
        let liked_at = sqlx::query_scalar::<_, String>(
            r#"
            SELECT to_char(
                liked_at AT TIME ZONE 'UTC',
                'YYYY-MM-DD"T"HH24:MI:SS.US"Z"'
            )
            FROM likes
            WHERE user_id = $1
              AND content_type = $2
              AND content_id = $3
            LIMIT 1
            "#,
        )
        .bind(user_id.to_string())
        .bind(content_type.to_string())
        .bind(content_id.to_string())
        .fetch_optional(self.db_pool)
        .await?;

        Ok(LikeStatus {
            exists: liked_at.is_some(),
            liked_at,
        })
    }

    pub async fn get_like_count(
        &self,
        content_type: &ContentType,
        content_id: &ContentId,
    ) -> Result<LikeCount, AppError> {
        let like_count = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT like_count
            FROM like_counts
            WHERE content_type = $1
              AND content_id = $2
            LIMIT 1
            "#,
        )
        .bind(content_type.to_string())
        .bind(content_id.to_string())
        .fetch_optional(self.db_pool)
        .await?
        .unwrap_or(0);

        Ok(LikeCount { count: like_count })
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
    pub liked_at: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LikeCount {
    pub count: i64,
}
