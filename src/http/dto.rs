use crate::domain::{ContentId, ContentType};
use crate::use_cases::{LikeContentResult, UnlikeContentResult};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
pub(crate) struct CreateLikeRequest {
    pub(crate) content_type: String,
    pub(crate) content_id: String,
}

#[derive(Deserialize)]
pub(crate) struct BatchLikesRequest {
    pub(crate) items: Vec<BatchLikeItemRequest>,
}

#[derive(Deserialize)]
pub(crate) struct BatchLikeItemRequest {
    pub(crate) content_type: String,
    pub(crate) content_id: String,
}

#[derive(Deserialize)]
pub(crate) struct UserLikesQuery {
    pub(crate) limit: Option<usize>,
    pub(crate) cursor: Option<String>,
    pub(crate) content_type: Option<String>,
}

#[derive(Clone, Deserialize)]
pub(crate) struct TopLikesQuery {
    pub(crate) window: Option<String>,
    pub(crate) limit: Option<usize>,
    pub(crate) content_type: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct LikeEventsStreamQuery {
    pub(crate) content_type: String,
    pub(crate) content_id: String,
}

#[derive(Serialize)]
pub(crate) struct LikeResponse {
    pub(crate) liked: bool,
    pub(crate) already_existed: bool,
    pub(crate) count: i64,
    pub(crate) liked_at: Option<String>,
}

impl From<LikeContentResult> for LikeResponse {
    fn from(value: LikeContentResult) -> Self {
        Self {
            liked: value.liked,
            already_existed: value.already_existed,
            count: value.count,
            liked_at: value.liked_at,
        }
    }
}

#[derive(Serialize)]
pub(crate) struct UnlikeResponse {
    pub(crate) liked: bool,
    pub(crate) was_liked: bool,
    pub(crate) count: i64,
}

impl From<UnlikeContentResult> for UnlikeResponse {
    fn from(value: UnlikeContentResult) -> Self {
        Self {
            liked: value.liked,
            was_liked: value.was_liked,
            count: value.count,
        }
    }
}

#[derive(Serialize)]
pub(crate) struct LikeStatusResponse {
    pub(crate) liked: bool,
    pub(crate) liked_at: Option<String>,
}

impl From<crate::storage::likes_repository::LikeStatus> for LikeStatusResponse {
    fn from(value: crate::storage::likes_repository::LikeStatus) -> Self {
        Self {
            liked: value.exists,
            liked_at: value.liked_at,
        }
    }
}

#[derive(Serialize)]
pub(crate) struct LikeCountResponse {
    pub(crate) content_type: String,
    pub(crate) content_id: String,
    pub(crate) count: i64,
}

impl LikeCountResponse {
    pub(crate) fn from_parts(
        content_type: &ContentType,
        content_id: &ContentId,
        count: i64,
    ) -> Self {
        Self {
            content_type: content_type.to_string(),
            content_id: content_id.to_string(),
            count,
        }
    }
}

#[derive(Serialize)]
pub(crate) struct BatchLikeCountsResponse {
    pub(crate) results: Vec<BatchLikeCountItemResponse>,
}

#[derive(Serialize)]
pub(crate) struct BatchLikeCountItemResponse {
    pub(crate) content_type: String,
    pub(crate) content_id: String,
    pub(crate) count: i64,
}

#[derive(Serialize)]
pub(crate) struct BatchLikeStatusesResponse {
    pub(crate) results: Vec<BatchLikeStatusItemResponse>,
}

#[derive(Serialize)]
pub(crate) struct BatchLikeStatusItemResponse {
    pub(crate) content_type: String,
    pub(crate) content_id: String,
    pub(crate) liked: bool,
    pub(crate) liked_at: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct UserLikesResponse {
    pub(crate) items: Vec<UserLikeItemResponse>,
    pub(crate) next_cursor: Option<String>,
    pub(crate) has_more: bool,
}

#[derive(Serialize)]
pub(crate) struct UserLikeItemResponse {
    pub(crate) content_type: String,
    pub(crate) content_id: String,
    pub(crate) liked_at: String,
}

#[derive(Serialize)]
pub(crate) struct TopLikesResponse {
    pub(crate) window: String,
    pub(crate) content_type: Option<String>,
    pub(crate) items: Vec<TopLikeItemResponse>,
}

#[derive(Serialize)]
pub(crate) struct TopLikeItemResponse {
    pub(crate) content_type: String,
    pub(crate) content_id: String,
    pub(crate) count: i64,
}
