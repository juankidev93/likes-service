use crate::domain::{ContentId, ContentType, UserId};
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

#[derive(Clone)]
pub struct LikeEvents {
    redis_client: redis::Client,
}

impl LikeEvents {
    pub fn new(redis_client: redis::Client) -> Self {
        Self { redis_client }
    }

    pub async fn subscribe(
        &self,
        content_type: &ContentType,
        content_id: &ContentId,
    ) -> redis::RedisResult<redis::aio::PubSub> {
        let mut pubsub = self.redis_client.get_async_pubsub().await?;
        pubsub.subscribe(channel_name(content_type, content_id)).await?;
        Ok(pubsub)
    }

    pub async fn publish_like(
        &self,
        user_id: &UserId,
        content_type: &ContentType,
        content_id: &ContentId,
        count: i64,
        liked_at: Option<&str>,
    ) -> redis::RedisResult<()> {
        self.publish_event(content_type, content_id, LikeEvent {
            event: "like".to_string(),
            user_id: user_id.as_external_id(),
            content_type: content_type.to_string(),
            content_id: content_id.to_string(),
            count,
            timestamp: liked_at
                .map(ToOwned::to_owned)
                .unwrap_or_else(current_timestamp),
        })
        .await
    }

    pub async fn publish_unlike(
        &self,
        user_id: &UserId,
        content_type: &ContentType,
        content_id: &ContentId,
        count: i64,
    ) -> redis::RedisResult<()> {
        self.publish_event(content_type, content_id, LikeEvent {
            event: "unlike".to_string(),
            user_id: user_id.as_external_id(),
            content_type: content_type.to_string(),
            content_id: content_id.to_string(),
            count,
            timestamp: current_timestamp(),
        })
        .await
    }

    async fn publish_event(
        &self,
        content_type: &ContentType,
        content_id: &ContentId,
        event: LikeEvent,
    ) -> redis::RedisResult<()> {
        let payload = serde_json::to_string(&event).map_err(|_| {
            redis::RedisError::from((redis::ErrorKind::TypeError, "failed to serialize like event"))
        })?;

        let mut connection = self.redis_client.get_multiplexed_async_connection().await?;
        let _: i64 = connection
            .publish(channel_name(content_type, content_id), payload)
            .await?;

        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LikeEvent {
    pub event: String,
    pub user_id: String,
    pub content_type: String,
    pub content_id: String,
    pub count: i64,
    pub timestamp: String,
}

pub fn current_timestamp() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .expect("current timestamp must format as RFC3339")
}

pub fn channel_name(content_type: &ContentType, content_id: &ContentId) -> String {
    format!("likes:events:{content_type}:{content_id}")
}
