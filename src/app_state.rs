use std::collections::HashMap;

use crate::profile_api_client::ProfileApiClient;
use redis::Client as RedisClient;
use sqlx::PgPool;

#[allow(dead_code)]
#[derive(Clone)]
pub struct AppState {
    pub db_pool: PgPool,
    pub redis_client: RedisClient,
    pub mock_profiles: HashMap<String, MockProfile>,
    pub profile_api_client: ProfileApiClient,
}

#[derive(Clone)]
pub struct MockProfile {
    pub user_id: String,
    pub display_name: String,
}
