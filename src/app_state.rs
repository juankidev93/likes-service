use std::collections::{HashMap, HashSet};

use crate::infra::shutdown::ShutdownSignal;
use crate::integrations::content_registry::ContentTypeRegistry;
use crate::integrations::content_validation::ContentValidationClient;
use crate::integrations::profile_api_client::ProfileApiClient;
use crate::integrations::sse_events::LikeEvents;
use redis::Client as RedisClient;
use sqlx::PgPool;

#[allow(dead_code)]
#[derive(Clone)]
pub struct AppState {
    pub db_pool: PgPool,
    pub read_db_pool: PgPool,
    pub redis_client: RedisClient,
    pub cache_ttl_like_counts_seconds: u64,
    pub write_rate_limit_per_minute: u32,
    pub read_rate_limit_per_minute: u32,
    pub sse_heartbeat_interval_seconds: u64,
    pub mock_profiles: HashMap<String, MockProfile>,
    pub mock_content_store: HashMap<String, HashSet<String>>,
    pub content_type_registry: ContentTypeRegistry,
    pub content_validation_client: ContentValidationClient,
    pub profile_api_client: ProfileApiClient,
    pub shutdown_signal: ShutdownSignal,
    pub like_events: LikeEvents,
}

#[derive(Clone)]
pub struct MockProfile {
    pub user_id: String,
    pub display_name: String,
}
