use std::collections::HashMap;

use redis::Client as RedisClient;
use sqlx::PgPool;

#[allow(dead_code)]
#[derive(Clone)]
pub struct AppState {
    pub db_pool: PgPool,
    pub redis_client: RedisClient,
    pub mock_profiles: HashMap<String, MockProfile>,
}

#[derive(Clone)]
pub struct MockProfile {
    pub user_id: String,
    pub display_name: String,
}
