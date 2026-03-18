use std::env;

pub struct ServiceConfig {
    pub host: String,
    pub port: u16,
    pub database_url: String,
    pub read_database_url: String,
    pub redis_url: String,
    pub db_max_connections: u32,
    pub db_min_connections: u32,
    pub db_acquire_timeout_secs: u64,
    #[allow(dead_code)]
    pub redis_pool_size: u32,
    pub write_rate_limit_per_minute: u32,
    pub read_rate_limit_per_minute: u32,
    pub cache_ttl_like_counts_seconds: u64,
    pub cache_ttl_content_validation_seconds: u64,
    #[allow(dead_code)]
    pub cache_ttl_user_status_seconds: u64,
    pub circuit_breaker_failure_threshold: u32,
    pub circuit_breaker_open_seconds: u64,
    pub circuit_breaker_success_threshold: u32,
    pub circuit_breaker_failure_window_seconds: u64,
    pub shutdown_timeout_secs: u64,
    pub sse_heartbeat_interval_seconds: u64,
    #[allow(dead_code)]
    pub leaderboard_refresh_interval_seconds: u64,
    pub profile_api_base_url: String,
    pub post_content_api_base_url: String,
    pub bonus_hunter_content_api_base_url: String,
    pub top_picks_content_api_base_url: String,
}

impl ServiceConfig {
    pub fn from_env() -> Result<Self, String> {
        let host = env::var("SERVICE_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
        let port = get_required_parsed::<u16>("HTTP_PORT")?;

        if host.trim().is_empty() {
            return Err("SERVICE_HOST cannot be empty".to_string());
        }

        let database_url = get_required_string("DATABASE_URL")?;
        let read_database_url = get_required_string("READ_DATABASE_URL")?;

        if database_url.trim().is_empty() {
            return Err("DATABASE_URL cannot be empty".to_string());
        }
        if read_database_url.trim().is_empty() {
            return Err("READ_DATABASE_URL cannot be empty".to_string());
        }

        let redis_url = get_required_string("REDIS_URL")?;

        if redis_url.trim().is_empty() {
            return Err("REDIS_URL cannot be empty".to_string());
        }

        let db_max_connections = get_optional_parsed::<u32>("DB_MAX_CONNECTIONS")?.unwrap_or(20);
        let db_min_connections = get_optional_parsed::<u32>("DB_MIN_CONNECTIONS")?.unwrap_or(5);
        let db_acquire_timeout_secs =
            get_optional_parsed::<u64>("DB_ACQUIRE_TIMEOUT_SECS")?.unwrap_or(5);
        let redis_pool_size = get_optional_parsed::<u32>("REDIS_POOL_SIZE")?.unwrap_or(10);

        let write_rate_limit_per_minute = get_optional_parsed::<u32>("RATE_LIMIT_WRITE_PER_MINUTE")?
        .unwrap_or(30);

        let read_rate_limit_per_minute = get_optional_parsed::<u32>("RATE_LIMIT_READ_PER_MINUTE")?
        .unwrap_or(1000);

        let cache_ttl_like_counts_seconds =
            get_optional_parsed::<u64>("CACHE_TTL_LIKE_COUNTS_SECS")?.unwrap_or(300);

        let cache_ttl_content_validation_seconds =
            get_optional_parsed::<u64>("CACHE_TTL_CONTENT_VALIDATION_SECS")?
        .unwrap_or(3600);
        let cache_ttl_user_status_seconds =
            get_optional_parsed::<u64>("CACHE_TTL_USER_STATUS_SECS")?.unwrap_or(60);

        let circuit_breaker_failure_threshold =
            get_optional_parsed::<u32>("CIRCUIT_BREAKER_FAILURE_THRESHOLD")?.unwrap_or(5);

        let circuit_breaker_open_seconds =
            get_optional_parsed::<u64>("CIRCUIT_BREAKER_RECOVERY_TIMEOUT_SECS")?.unwrap_or(30);

        let circuit_breaker_success_threshold =
            get_optional_parsed::<u32>("CIRCUIT_BREAKER_SUCCESS_THRESHOLD")?.unwrap_or(3);

        let circuit_breaker_failure_window_seconds =
            get_optional_parsed::<u64>("CIRCUIT_BREAKER_FAILURE_WINDOW_SECONDS")?.unwrap_or(30);
        let shutdown_timeout_secs =
            get_optional_parsed::<u64>("SHUTDOWN_TIMEOUT_SECS")?.unwrap_or(30);

        let sse_heartbeat_interval_seconds =
            get_optional_parsed::<u64>("SSE_HEARTBEAT_INTERVAL_SECS")?.unwrap_or(15);
        let leaderboard_refresh_interval_seconds =
            get_optional_parsed::<u64>("LEADERBOARD_REFRESH_INTERVAL_SECS")?.unwrap_or(60);

        let profile_api_base_url = get_required_string("PROFILE_API_URL")?;

        if profile_api_base_url.trim().is_empty() {
            return Err("PROFILE_API_URL cannot be empty".to_string());
        }

        let post_content_api_base_url = get_required_string("CONTENT_API_POST_URL")?;
        let bonus_hunter_content_api_base_url =
            get_required_string("CONTENT_API_BONUS_HUNTER_URL")?;
        let top_picks_content_api_base_url = get_required_string("CONTENT_API_TOP_PICKS_URL")?;

        if post_content_api_base_url.trim().is_empty() {
            return Err("CONTENT_API_POST_URL cannot be empty".to_string());
        }

        if bonus_hunter_content_api_base_url.trim().is_empty() {
            return Err("CONTENT_API_BONUS_HUNTER_URL cannot be empty".to_string());
        }

        if top_picks_content_api_base_url.trim().is_empty() {
            return Err("CONTENT_API_TOP_PICKS_URL cannot be empty".to_string());
        }

        Ok(Self {
            host,
            port,
            database_url,
            read_database_url,
            redis_url,
            db_max_connections,
            db_min_connections,
            db_acquire_timeout_secs,
            redis_pool_size,
            write_rate_limit_per_minute,
            read_rate_limit_per_minute,
            cache_ttl_like_counts_seconds,
            cache_ttl_content_validation_seconds,
            cache_ttl_user_status_seconds,
            circuit_breaker_failure_threshold,
            circuit_breaker_open_seconds,
            circuit_breaker_success_threshold,
            circuit_breaker_failure_window_seconds,
            shutdown_timeout_secs,
            sse_heartbeat_interval_seconds,
            leaderboard_refresh_interval_seconds,
            profile_api_base_url,
            post_content_api_base_url,
            bonus_hunter_content_api_base_url,
            top_picks_content_api_base_url,
        })
    }

    pub fn bind_address(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

fn get_required_string(primary: &str) -> Result<String, String> {
    get_optional_string(primary)
        .ok_or_else(|| format!("{primary} is required"))
}

fn get_required_parsed<T>(primary: &str) -> Result<T, String>
where
    T: std::str::FromStr,
{
    get_optional_parsed(primary)?.ok_or_else(|| format!("{primary} is required"))
}

fn get_optional_string(primary: &str) -> Option<String> {
    env::var(primary).ok()
}

fn get_optional_parsed<T>(primary: &str) -> Result<Option<T>, String>
where
    T: std::str::FromStr,
{
    match get_optional_string(primary) {
        Some(value) => value
            .parse::<T>()
            .map(Some)
            .map_err(|_| format!("{primary} must be a valid value, got '{value}'")),
        None => Ok(None),
    }
}
