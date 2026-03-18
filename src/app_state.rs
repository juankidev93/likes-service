use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use crate::infra::shutdown::ShutdownSignal;
use crate::integrations::content_registry::ContentTypeRegistry;
use crate::integrations::content_validation::ContentValidationClient;
use crate::integrations::profile_api_client::ProfileApiClient;
use crate::integrations::sse_events::LikeEvents;
use redis::Client as RedisClient;
use redis::aio::MultiplexedConnection;
use sqlx::PgPool;
use tokio::sync::{Mutex, Notify};

#[allow(dead_code)]
#[derive(Clone)]
pub struct AppState {
    pub db_pool: PgPool,
    pub read_db_pool: PgPool,
    pub redis_client: RedisClient,
    pub redis_connection: Option<MultiplexedConnection>,
    pub cache_ttl_like_counts_seconds: u64,
    pub cache_ttl_user_status_seconds: u64,
    pub leaderboard_refresh_interval_seconds: u64,
    pub write_rate_limit_per_minute: u32,
    pub read_rate_limit_per_minute: u32,
    pub sse_heartbeat_interval_seconds: u64,
    pub local_like_count_cache: Arc<LocalLikeCountCache>,
    pub like_count_cache_inflight: Arc<Mutex<HashMap<String, Arc<Notify>>>>,
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

#[derive(Default)]
pub struct LocalLikeCountCache {
    entries: RwLock<HashMap<String, LocalLikeCountCacheEntry>>,
}

#[derive(Clone, Copy)]
struct LocalLikeCountCacheEntry {
    count: i64,
    expires_at: Instant,
}

impl AppState {
    pub async fn get_redis_connection(&self) -> Result<MultiplexedConnection, redis::RedisError> {
        if let Some(connection) = self.redis_connection.clone() {
            return Ok(connection);
        }

        self.redis_client.get_multiplexed_async_connection().await
    }

    pub async fn begin_like_count_fill(&self, key: &str) -> LikeCountFillPermit {
        let mut inflight = self.like_count_cache_inflight.lock().await;

        if let Some(notify) = inflight.get(key) {
            return LikeCountFillPermit::Follower(notify.clone());
        }

        let notify = Arc::new(Notify::new());
        inflight.insert(key.to_string(), notify.clone());

        LikeCountFillPermit::Leader(notify)
    }

    pub async fn finish_like_count_fill(&self, key: &str, notify: &Arc<Notify>) {
        let mut inflight = self.like_count_cache_inflight.lock().await;

        if inflight
            .get(key)
            .is_some_and(|current| Arc::ptr_eq(current, notify))
        {
            inflight.remove(key);
        }

        notify.notify_waiters();
    }
}

pub enum LikeCountFillPermit {
    Leader(Arc<Notify>),
    Follower(Arc<Notify>),
}

impl LocalLikeCountCache {
    pub fn get(&self, key: &str) -> Option<i64> {
        let now = Instant::now();
        let entry = self.entries.read().ok()?.get(key).copied()?;

        if entry.expires_at > now {
            return Some(entry.count);
        }

        if let Ok(mut entries) = self.entries.write() {
            if entries
                .get(key)
                .is_some_and(|entry| entry.expires_at <= now)
            {
                entries.remove(key);
            }
        }

        None
    }

    pub fn set(&self, key: String, count: i64, ttl: Duration) {
        let expires_at = Instant::now() + ttl;

        if let Ok(mut entries) = self.entries.write() {
            entries.insert(key, LocalLikeCountCacheEntry { count, expires_at });
        }
    }
}
