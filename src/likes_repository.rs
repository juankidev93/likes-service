#![allow(dead_code)]

use crate::domain::{ContentId, ContentType, UserId};
use crate::error::AppError;
use sqlx::PgPool;
use std::collections::HashMap;

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

    pub async fn get_like_counts_batch(
        &self,
        items: &[(ContentType, ContentId)],
    ) -> Result<HashMap<(String, String), i64>, AppError> {
        if items.is_empty() {
            return Ok(HashMap::new());
        }

        let content_types: Vec<String> = items
            .iter()
            .map(|(content_type, _)| content_type.to_string())
            .collect();
        let content_ids: Vec<String> = items
            .iter()
            .map(|(_, content_id)| content_id.to_string())
            .collect();

        let rows: Vec<(String, String, i64)> = sqlx::query_as(
            r#"
            SELECT requested.content_type, requested.content_id, COALESCE(like_counts.like_count, 0)
            FROM UNNEST($1::text[], $2::text[]) AS requested(content_type, content_id)
            LEFT JOIN like_counts
              ON like_counts.content_type = requested.content_type
             AND like_counts.content_id = requested.content_id
            "#,
        )
            .bind(content_types)
            .bind(content_ids)
            .fetch_all(self.db_pool)
            .await?;

        Ok(rows
            .into_iter()
            .map(|(content_type, content_id, like_count)| {
                ((content_type, content_id), like_count)
            })
            .collect())
    }

    pub async fn get_like_statuses_batch(
        &self,
        user_id: &UserId,
        items: &[(ContentType, ContentId)],
    ) -> Result<HashMap<(String, String), LikeStatus>, AppError> {
        if items.is_empty() {
            return Ok(HashMap::new());
        }

        let content_types: Vec<String> = items
            .iter()
            .map(|(content_type, _)| content_type.to_string())
            .collect();
        let content_ids: Vec<String> = items
            .iter()
            .map(|(_, content_id)| content_id.to_string())
            .collect();

        let rows: Vec<(String, String, Option<String>)> = sqlx::query_as(
            r#"
            SELECT requested.content_type, requested.content_id,
                   to_char(likes.liked_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US"Z"')
            FROM UNNEST($2::text[], $3::text[]) AS requested(content_type, content_id)
            LEFT JOIN likes
              ON likes.user_id = $1
             AND likes.content_type = requested.content_type
             AND likes.content_id = requested.content_id
            "#,
        )
            .bind(user_id.to_string())
            .bind(content_types)
            .bind(content_ids)
            .fetch_all(self.db_pool)
            .await?;

        Ok(rows
            .into_iter()
            .map(|(content_type, content_id, liked_at)| {
                (
                    (content_type, content_id),
                    LikeStatus {
                        exists: liked_at.is_some(),
                        liked_at,
                    },
                )
            })
            .collect())
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
