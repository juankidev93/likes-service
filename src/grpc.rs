use crate::app_state::AppState;
use crate::domain::{ContentId, ContentType as DomainContentType, UserId};
use crate::error::AppError;
use crate::http::helpers::{
    decode_cursor, encode_cursor, parse_limit, parse_top_likes_limit, parse_top_likes_window,
};
use crate::integrations::profile_api_client::{AuthError, AuthenticatedUser};
use crate::storage::likes_repository::{PostgresLikesRepository, TopLikesWindow};
use crate::use_cases::LikesUseCases;
use std::str::FromStr;
use tonic::{Request, Response, Status};

pub mod pb {
    tonic::include_proto!("social.likes.v1");
}

pub const FILE_DESCRIPTOR_SET: &[u8] = tonic::include_file_descriptor_set!("likes_descriptor");

use pb::likes_service_server::{LikesService, LikesServiceServer};
use pb::*;

#[derive(Clone)]
pub struct GrpcLikesService {
    state: AppState,
}

impl GrpcLikesService {
    pub fn new(state: AppState) -> Self {
        Self { state }
    }

    pub fn into_server(self) -> LikesServiceServer<Self> {
        LikesServiceServer::new(self)
    }

    async fn authenticate(&self, token: &str) -> Result<AuthenticatedUser, Status> {
        let token = token.trim();
        let token = token
            .strip_prefix("Bearer ")
            .unwrap_or(token)
            .trim()
            .to_string();

        self.state
            .profile_api_client
            .validate_token(&token)
            .await
            .map_err(auth_error_to_status)
    }
}

#[tonic::async_trait]
impl LikesService for GrpcLikesService {
    async fn like(&self, request: Request<LikeRequest>) -> Result<Response<LikeResponse>, Status> {
        let request = request.into_inner();
        let authenticated_user = self.authenticate(&request.session_token).await?;
        let user_id = parse_authenticated_user_id(&authenticated_user)?;
        let content_type = parse_content_type(request.content_type)?;
        let content_id = parse_content_id(&request.content_id)?;

        let repository = PostgresLikesRepository::new(&self.state.db_pool);
        let use_cases = LikesUseCases::new(
            repository,
            self.state.redis_client.clone(),
            self.state.content_validation_client.clone(),
            self.state.cache_ttl_like_counts_seconds,
        );

        let result = use_cases
            .like_content(&user_id, &content_type, &content_id)
            .await
            .map_err(app_error_to_status)?;

        Ok(Response::new(LikeResponse {
            liked: result.liked,
            already_existed: result.already_existed,
            count: result.count,
            liked_at: result.liked_at,
        }))
    }

    async fn unlike(
        &self,
        request: Request<UnlikeRequest>,
    ) -> Result<Response<UnlikeResponse>, Status> {
        let request = request.into_inner();
        let authenticated_user = self.authenticate(&request.session_token).await?;
        let user_id = parse_authenticated_user_id(&authenticated_user)?;
        let content_type = parse_content_type(request.content_type)?;
        let content_id = parse_content_id(&request.content_id)?;

        let repository = PostgresLikesRepository::new(&self.state.db_pool);
        let use_cases = LikesUseCases::new(
            repository,
            self.state.redis_client.clone(),
            self.state.content_validation_client.clone(),
            self.state.cache_ttl_like_counts_seconds,
        );

        let result = use_cases
            .unlike_content(&user_id, &content_type, &content_id)
            .await
            .map_err(app_error_to_status)?;

        Ok(Response::new(UnlikeResponse {
            liked: result.liked,
            was_liked: result.was_liked,
            count: result.count,
        }))
    }

    async fn get_like_count(
        &self,
        request: Request<GetLikeCountRequest>,
    ) -> Result<Response<GetLikeCountResponse>, Status> {
        let request = request.into_inner();
        let content_type = parse_content_type(request.content_type)?;
        let content_id = parse_content_id(&request.content_id)?;
        let repository = PostgresLikesRepository::new(&self.state.read_db_pool);
        let count = repository
            .get_like_count(&content_type, &content_id)
            .await
            .map_err(app_error_to_status)?;

        Ok(Response::new(GetLikeCountResponse {
            content_type: request.content_type,
            content_id: request.content_id,
            count: count.count,
        }))
    }

    async fn get_like_status(
        &self,
        request: Request<GetLikeStatusRequest>,
    ) -> Result<Response<GetLikeStatusResponse>, Status> {
        let request = request.into_inner();
        let authenticated_user = self.authenticate(&request.session_token).await?;
        let user_id = parse_authenticated_user_id(&authenticated_user)?;
        let content_type = parse_content_type(request.content_type)?;
        let content_id = parse_content_id(&request.content_id)?;
        let repository = PostgresLikesRepository::new(&self.state.read_db_pool);
        let status = repository
            .get_like_status(&user_id, &content_type, &content_id)
            .await
            .map_err(app_error_to_status)?;

        Ok(Response::new(GetLikeStatusResponse {
            liked: status.exists,
            liked_at: status.liked_at,
        }))
    }

    async fn get_user_likes(
        &self,
        request: Request<GetUserLikesRequest>,
    ) -> Result<Response<GetUserLikesResponse>, Status> {
        let request = request.into_inner();
        let authenticated_user = self.authenticate(&request.session_token).await?;
        let user_id = parse_authenticated_user_id(&authenticated_user)?;
        let limit = parse_limit(request.limit.map(|value| value as usize)).map_err(app_error_to_status)?;
        let cursor = request.cursor.as_deref().map(decode_cursor).transpose().map_err(app_error_to_status)?;
        let content_type = request
            .content_type
            .map(parse_content_type)
            .transpose()?;

        let repository = PostgresLikesRepository::new(&self.state.read_db_pool);
        let rows = repository
            .list_user_likes(&user_id, content_type.as_ref(), cursor.as_ref(), limit + 1)
            .await
            .map_err(app_error_to_status)?;

        let has_more = rows.len() > limit;
        let mut page_rows = rows;
        if has_more {
            page_rows.truncate(limit);
        }

        let next_cursor = if has_more {
            page_rows.last().map(encode_cursor)
        } else {
            None
        };

        Ok(Response::new(GetUserLikesResponse {
            items: page_rows
                .into_iter()
                .map(|row| UserLikeItem {
                    content_type: content_type_to_proto(&row.content_type),
                    content_id: row.content_id,
                    liked_at: row.liked_at,
                })
                .collect(),
            next_cursor,
            has_more,
        }))
    }

    async fn batch_get_like_counts(
        &self,
        request: Request<BatchGetLikeCountsRequest>,
    ) -> Result<Response<BatchGetLikeCountsResponse>, Status> {
        let request = request.into_inner();
        let items = parse_content_refs(&request.items)?;
        let repository = PostgresLikesRepository::new(&self.state.read_db_pool);
        let counts = repository
            .get_like_counts_batch(&items)
            .await
            .map_err(app_error_to_status)?;

        Ok(Response::new(BatchGetLikeCountsResponse {
            results: items
                .into_iter()
                .map(|(content_type, content_id)| {
                    let key = (content_type.to_string(), content_id.to_string());
                    BatchLikeCountItem {
                        content_type: content_type_to_proto(content_type.as_str()),
                        content_id: content_id.to_string(),
                        count: *counts.get(&key).unwrap_or(&0),
                    }
                })
                .collect(),
        }))
    }

    async fn batch_get_like_statuses(
        &self,
        request: Request<BatchGetLikeStatusesRequest>,
    ) -> Result<Response<BatchGetLikeStatusesResponse>, Status> {
        let request = request.into_inner();
        let authenticated_user = self.authenticate(&request.session_token).await?;
        let user_id = parse_authenticated_user_id(&authenticated_user)?;
        let items = parse_content_refs(&request.items)?;
        let repository = PostgresLikesRepository::new(&self.state.read_db_pool);
        let statuses = repository
            .get_like_statuses_batch(&user_id, &items)
            .await
            .map_err(app_error_to_status)?;

        Ok(Response::new(BatchGetLikeStatusesResponse {
            results: items
                .into_iter()
                .map(|(content_type, content_id)| {
                    let key = (content_type.to_string(), content_id.to_string());
                    let status = statuses.get(&key);
                    BatchLikeStatusItem {
                        content_type: content_type_to_proto(content_type.as_str()),
                        content_id: content_id.to_string(),
                        liked: status.map(|value| value.exists).unwrap_or(false),
                        liked_at: status.and_then(|value| value.liked_at.clone()),
                    }
                })
                .collect(),
        }))
    }

    async fn get_top_likes(
        &self,
        request: Request<GetTopLikesRequest>,
    ) -> Result<Response<GetTopLikesResponse>, Status> {
        let request = request.into_inner();
        let window = parse_top_window(request.window)?;
        let limit =
            parse_top_likes_limit(request.limit.map(|value| value as usize)).map_err(app_error_to_status)?;
        let content_type = request.content_type.map(parse_content_type).transpose()?;
        let repository = PostgresLikesRepository::new(&self.state.read_db_pool);
        let rows = repository
            .list_top_likes(content_type.as_ref(), &window, limit)
            .await
            .map_err(app_error_to_status)?;

        Ok(Response::new(GetTopLikesResponse {
            window: top_window_to_proto(&window),
            content_type: content_type.as_ref().map(|value| content_type_to_proto(value.as_str())),
            items: rows
                .into_iter()
                .map(|row| TopLikeItem {
                    content_type: content_type_to_proto(&row.content_type),
                    content_id: row.content_id,
                    count: row.like_count,
                })
                .collect(),
        }))
    }
}

fn parse_authenticated_user_id(user: &AuthenticatedUser) -> Result<UserId, Status> {
    UserId::from_str(&user.user_id).map_err(app_error_to_status)
}

fn parse_content_id(content_id: &str) -> Result<ContentId, Status> {
    ContentId::from_str(content_id).map_err(app_error_to_status)
}

fn parse_content_type(value: i32) -> Result<DomainContentType, Status> {
    let proto = pb::ContentType::try_from(value)
        .map_err(|_| Status::invalid_argument("invalid content_type enum"))?;

    let as_str = match proto {
        pb::ContentType::Post => "post",
        pb::ContentType::BonusHunter => "bonus_hunter",
        pb::ContentType::TopPicks => "top_picks",
        pb::ContentType::Unspecified => {
            return Err(Status::invalid_argument("content_type is required"));
        }
    };

    DomainContentType::from_str(as_str).map_err(app_error_to_status)
}

fn parse_content_refs(items: &[ContentRef]) -> Result<Vec<(DomainContentType, ContentId)>, Status> {
    items.iter()
        .map(|item| Ok((parse_content_type(item.content_type)?, parse_content_id(&item.content_id)?)))
        .collect()
}

fn parse_top_window(value: i32) -> Result<TopLikesWindow, Status> {
    let proto = pb::TopLikesWindow::try_from(value)
        .map_err(|_| Status::invalid_argument("invalid top likes window enum"))?;

    match proto {
        pb::TopLikesWindow::TopLikesWindow24h => Ok(TopLikesWindow::Last24Hours),
        pb::TopLikesWindow::TopLikesWindow7d => Ok(TopLikesWindow::Last7Days),
        pb::TopLikesWindow::TopLikesWindow30d => Ok(TopLikesWindow::Last30Days),
        pb::TopLikesWindow::All => Ok(TopLikesWindow::All),
        pb::TopLikesWindow::Unspecified => parse_top_likes_window(None).map_err(app_error_to_status),
    }
}

fn top_window_to_proto(window: &TopLikesWindow) -> i32 {
    match window {
        TopLikesWindow::Last24Hours => pb::TopLikesWindow::TopLikesWindow24h as i32,
        TopLikesWindow::Last7Days => pb::TopLikesWindow::TopLikesWindow7d as i32,
        TopLikesWindow::Last30Days => pb::TopLikesWindow::TopLikesWindow30d as i32,
        TopLikesWindow::All => pb::TopLikesWindow::All as i32,
    }
}

fn content_type_to_proto(value: &str) -> i32 {
    match value {
        "post" => pb::ContentType::Post as i32,
        "bonus_hunter" => pb::ContentType::BonusHunter as i32,
        "top_picks" => pb::ContentType::TopPicks as i32,
        _ => pb::ContentType::Unspecified as i32,
    }
}

fn app_error_to_status(error: impl Into<AppError>) -> Status {
    match error.into() {
        AppError::InvalidRequest { code, message } => Status::invalid_argument(format!("{code}: {message}")),
        AppError::Unauthorized { code, message } => Status::unauthenticated(format!("{code}: {message}")),
        AppError::DependencyUnavailable { code, message } => Status::unavailable(format!("{code}: {message}")),
        AppError::Domain(error) => Status::invalid_argument(error.to_string()),
        AppError::ContentValidation(error) => match AppError::from(error) {
            AppError::InvalidRequest { code, message } => Status::invalid_argument(format!("{code}: {message}")),
            AppError::DependencyUnavailable { code, message } => Status::unavailable(format!("{code}: {message}")),
            other => Status::internal(other.to_string()),
        },
        AppError::Database(error) => Status::internal(error.to_string()),
        AppError::Cache(error) => Status::internal(error.to_string()),
    }
}

fn auth_error_to_status(error: AuthError) -> Status {
    match error {
        AuthError::InvalidToken => Status::unauthenticated("UNAUTHORIZED: invalid token"),
        AuthError::DependencyUnavailable(message) => {
            Status::unavailable(format!("DEPENDENCY_UNAVAILABLE: {message}"))
        }
        AuthError::NetworkError(message) => Status::unavailable(format!("DEPENDENCY_UNAVAILABLE: {message}")),
    }
}
