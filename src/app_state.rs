use std::collections::{HashMap, HashSet};

use crate::content_validation::ContentValidationClient;
use crate::content_registry::ContentTypeRegistry;
use crate::profile_api_client::ProfileApiClient;
use redis::Client as RedisClient;
use sqlx::PgPool;

#[allow(dead_code)]
#[derive(Clone)]
pub struct AppState {
    pub db_pool: PgPool,
    pub redis_client: RedisClient,
    pub write_rate_limit_per_minute: u32,
    pub mock_profiles: HashMap<String, MockProfile>,
    pub mock_content_store: HashMap<String, HashSet<String>>,
    pub content_type_registry: ContentTypeRegistry,
    pub content_validation_client: ContentValidationClient,
    pub profile_api_client: ProfileApiClient,
}

#[derive(Clone)]
pub struct MockProfile {
    pub user_id: String,
    pub display_name: String,
}
