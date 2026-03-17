use crate::domain::{ContentId, ContentType, UserId};
use time::{format_description::well_known::Rfc3339, OffsetDateTime};
use tokio::sync::broadcast;

const LIKE_EVENTS_BUFFER: usize = 256;

#[derive(Clone)]
pub struct LikeEvents {
    sender: broadcast::Sender<LikeEvent>,
}

impl LikeEvents {
    pub fn new() -> Self {
        let (sender, _) = broadcast::channel(LIKE_EVENTS_BUFFER);
        Self { sender }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<LikeEvent> {
        self.sender.subscribe()
    }

    pub fn publish_like(
        &self,
        user_id: &UserId,
        content_type: &ContentType,
        content_id: &ContentId,
        count: i64,
        liked_at: Option<&str>,
    ) {
        let _ = self.sender.send(LikeEvent {
            event: "like".to_string(),
            user_id: user_id.to_string(),
            content_type: content_type.to_string(),
            content_id: content_id.to_string(),
            count,
            timestamp: liked_at
                .map(ToOwned::to_owned)
                .unwrap_or_else(current_timestamp),
        });
    }

    pub fn publish_unlike(
        &self,
        user_id: &UserId,
        content_type: &ContentType,
        content_id: &ContentId,
        count: i64,
    ) {
        let _ = self.sender.send(LikeEvent {
            event: "unlike".to_string(),
            user_id: user_id.to_string(),
            content_type: content_type.to_string(),
            content_id: content_id.to_string(),
            count,
            timestamp: current_timestamp(),
        });
    }
}

#[derive(Clone, Debug)]
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
