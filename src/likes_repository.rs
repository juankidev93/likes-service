#![allow(dead_code)]

use crate::domain::{ContentId, ContentType, UserId};
use crate::error::AppError;
use sqlx::PgPool;
use std::{collections::HashMap, str::FromStr};

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
        let mut transaction = self.db_pool.begin().await?;

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
            .execute(&mut *transaction)
            .await?;

        let insert_result = if result.rows_affected() == 1 {
            sqlx::query(
                r#"
                INSERT INTO like_counts (content_type, content_id, like_count)
                VALUES ($1, $2, 1)
                ON CONFLICT (content_type, content_id)
                DO UPDATE SET like_count = like_counts.like_count + 1
                "#,
            )
            .bind(content_type.to_string())
            .bind(content_id.to_string())
            .execute(&mut *transaction)
            .await?;

            sqlx::query(
                r#"
                INSERT INTO like_hourly_counts (bucket_start, content_type, content_id, like_count)
                VALUES (
                    date_trunc('hour', NOW() AT TIME ZONE 'UTC') AT TIME ZONE 'UTC',
                    $1,
                    $2,
                    1
                )
                ON CONFLICT (bucket_start, content_type, content_id)
                DO UPDATE SET like_count = like_hourly_counts.like_count + 1
                "#,
            )
            .bind(content_type.to_string())
            .bind(content_id.to_string())
            .execute(&mut *transaction)
            .await?;

            InsertLikeResult::Inserted
        } else {
            InsertLikeResult::AlreadyExists
        };

        transaction.commit().await?;

        Ok(insert_result)
    }

    pub async fn delete_like(
        &self,
        user_id: &UserId,
        content_type: &ContentType,
        content_id: &ContentId,
    ) -> Result<DeleteLikeResult, AppError> {
        let mut transaction = self.db_pool.begin().await?;

        let deleted_bucket_start = sqlx::query_scalar::<_, String>(
            r#"
            DELETE FROM likes
            WHERE user_id = $1
              AND content_type = $2
              AND content_id = $3
            RETURNING (date_trunc('hour', liked_at AT TIME ZONE 'UTC') AT TIME ZONE 'UTC')::text
            "#,
        )
        .bind(user_id.to_string())
        .bind(content_type.to_string())
        .bind(content_id.to_string())
        .fetch_optional(&mut *transaction)
        .await?;

        let delete_result = if let Some(bucket_start) = deleted_bucket_start {
            sqlx::query(
                r#"
                INSERT INTO like_counts (content_type, content_id, like_count)
                VALUES ($1, $2, 0)
                ON CONFLICT (content_type, content_id)
                DO UPDATE SET like_count = GREATEST(like_counts.like_count - 1, 0)
                "#,
            )
            .bind(content_type.to_string())
            .bind(content_id.to_string())
            .execute(&mut *transaction)
            .await?;

            sqlx::query(
                r#"
                INSERT INTO like_hourly_counts (bucket_start, content_type, content_id, like_count)
                VALUES ($1::timestamptz, $2, $3, 0)
                ON CONFLICT (bucket_start, content_type, content_id)
                DO UPDATE SET like_count = GREATEST(like_hourly_counts.like_count - 1, 0)
                "#,
            )
            .bind(bucket_start)
            .bind(content_type.to_string())
            .bind(content_id.to_string())
            .execute(&mut *transaction)
            .await?;

            DeleteLikeResult::Deleted
        } else {
            DeleteLikeResult::NotFound
        };

        transaction.commit().await?;

        Ok(delete_result)
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

    pub async fn list_user_likes(
        &self,
        user_id: &UserId,
        content_type: Option<&ContentType>,
        cursor: Option<&LikesCursor>,
        limit: usize,
    ) -> Result<Vec<UserLikeRow>, AppError> {
        let rows: Vec<(String, String, String)> = match (content_type, cursor) {
            (Some(content_type), Some(cursor)) => {
                sqlx::query_as(
                    r#"
                    SELECT content_type, content_id,
                           to_char(liked_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US"Z"')
                    FROM likes
                    WHERE user_id = $1
                      AND content_type = $2
                      AND (
                        liked_at < $3::timestamptz
                        OR (liked_at = $3::timestamptz AND content_id < $4)
                      )
                    ORDER BY liked_at DESC, content_id DESC
                    LIMIT $5
                    "#,
                )
                .bind(user_id.to_string())
                .bind(content_type.to_string())
                .bind(cursor.liked_at.as_str())
                .bind(cursor.content_id.to_string())
                .bind(limit as i64)
                .fetch_all(self.db_pool)
                .await?
            }
            (Some(content_type), None) => {
                sqlx::query_as(
                    r#"
                    SELECT content_type, content_id,
                           to_char(liked_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US"Z"')
                    FROM likes
                    WHERE user_id = $1
                      AND content_type = $2
                    ORDER BY liked_at DESC, content_id DESC
                    LIMIT $3
                    "#,
                )
                .bind(user_id.to_string())
                .bind(content_type.to_string())
                .bind(limit as i64)
                .fetch_all(self.db_pool)
                .await?
            }
            (None, Some(cursor)) => {
                sqlx::query_as(
                    r#"
                    SELECT content_type, content_id,
                           to_char(liked_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US"Z"')
                    FROM likes
                    WHERE user_id = $1
                      AND (
                        liked_at < $2::timestamptz
                        OR (liked_at = $2::timestamptz AND content_id < $3)
                      )
                    ORDER BY liked_at DESC, content_id DESC
                    LIMIT $4
                    "#,
                )
                .bind(user_id.to_string())
                .bind(cursor.liked_at.as_str())
                .bind(cursor.content_id.to_string())
                .bind(limit as i64)
                .fetch_all(self.db_pool)
                .await?
            }
            (None, None) => {
                sqlx::query_as(
                    r#"
                    SELECT content_type, content_id,
                           to_char(liked_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US"Z"')
                    FROM likes
                    WHERE user_id = $1
                    ORDER BY liked_at DESC, content_id DESC
                    LIMIT $2
                    "#,
                )
                .bind(user_id.to_string())
                .bind(limit as i64)
                .fetch_all(self.db_pool)
                .await?
            }
        };

        Ok(rows
            .into_iter()
            .map(|(content_type, content_id, liked_at)| UserLikeRow {
                content_type,
                content_id,
                liked_at,
            })
            .collect())
    }

    pub async fn list_top_likes(
        &self,
        content_type: Option<&ContentType>,
        window: &TopLikesWindow,
        limit: usize,
    ) -> Result<Vec<TopLikeRow>, AppError> {
        let rows: Vec<(String, String, i64)> = match window {
            TopLikesWindow::All => {
                sqlx::query_as(
                    r#"
                    SELECT content_type, content_id, like_count
                    FROM like_counts
                    WHERE like_count > 0
                      AND ($1::text IS NULL OR content_type = $1)
                    ORDER BY like_count DESC, content_type DESC, content_id DESC
                    LIMIT $2
                    "#,
                )
                .bind(content_type.map(ToString::to_string))
                .bind(limit as i64)
                .fetch_all(self.db_pool)
                .await?
            }
            _ => {
                sqlx::query_as(
                    r#"
                    SELECT content_type, content_id, SUM(like_count)::bigint AS like_count
                    FROM like_hourly_counts
                    WHERE like_count > 0
                      AND ($1::text IS NULL OR content_type = $1)
                      AND bucket_start >= NOW() - ($2::interval)
                    GROUP BY content_type, content_id
                    HAVING SUM(like_count) > 0
                    ORDER BY like_count DESC, content_type DESC, content_id DESC
                    LIMIT $3
                    "#,
                )
                .bind(content_type.map(ToString::to_string))
                .bind(window.as_interval().expect("non-all windows must have an interval"))
                .bind(limit as i64)
                .fetch_all(self.db_pool)
                .await?
            }
        };

        Ok(rows
            .into_iter()
            .map(|(content_type, content_id, like_count)| TopLikeRow {
                content_type,
                content_id,
                like_count,
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LikesCursor {
    pub liked_at: String,
    pub content_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UserLikeRow {
    pub content_type: String,
    pub content_id: String,
    pub liked_at: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TopLikeRow {
    pub content_type: String,
    pub content_id: String,
    pub like_count: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TopLikesWindow {
    Last24Hours,
    Last7Days,
    Last30Days,
    All,
}

impl TopLikesWindow {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Last24Hours => "24h",
            Self::Last7Days => "7d",
            Self::Last30Days => "30d",
            Self::All => "all",
        }
    }

    fn as_interval(&self) -> Option<&'static str> {
        match self {
            Self::Last24Hours => Some("24 hours"),
            Self::Last7Days => Some("7 days"),
            Self::Last30Days => Some("30 days"),
            Self::All => None,
        }
    }
}

impl FromStr for TopLikesWindow {
    type Err = AppError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "24h" => Ok(Self::Last24Hours),
            "7d" => Ok(Self::Last7Days),
            "30d" => Ok(Self::Last30Days),
            "all" => Ok(Self::All),
            _ => Err(AppError::invalid_request(
                "INVALID_REQUEST",
                "window must be one of: 24h, 7d, 30d, all",
            )),
        }
    }
}
