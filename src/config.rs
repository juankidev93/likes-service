use std::env;

pub struct ServiceConfig {
    pub host: String,
    pub port: u16,
    pub database_url: String,
    pub redis_url: String,
    pub write_rate_limit_per_minute: u32,
    pub read_rate_limit_per_minute: u32,
    pub profile_api_base_url: String,
    pub post_content_api_base_url: String,
    pub bonus_hunter_content_api_base_url: String,
    pub top_picks_content_api_base_url: String,
}

impl ServiceConfig {
    pub fn from_env() -> Result<Self, String> {
        let host = env::var("SERVICE_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
        let port = match env::var("SERVICE_PORT") {
            Ok(value) => value
                .parse::<u16>()
                .map_err(|_| format!("SERVICE_PORT must be a valid u16 integer, got '{value}'"))?,
            Err(_) => 3000,
        };

        if host.trim().is_empty() {
            return Err("SERVICE_HOST cannot be empty".to_string());
        }

        let database_url = env::var("DATABASE_URL")
            .map_err(|_| "DATABASE_URL is required".to_string())?;

        if database_url.trim().is_empty() {
            return Err("DATABASE_URL cannot be empty".to_string());
        }

        let redis_url = env::var("REDIS_URL")
            .map_err(|_| "REDIS_URL is required".to_string())?;

        if redis_url.trim().is_empty() {
            return Err("REDIS_URL cannot be empty".to_string());
        }

        let write_rate_limit_per_minute =
            match env::var("WRITE_RATE_LIMIT_PER_MINUTE") {
                Ok(value) => value.parse::<u32>().map_err(|_| {
                    format!(
                        "WRITE_RATE_LIMIT_PER_MINUTE must be a valid u32 integer, got '{value}'"
                    )
                })?,
                Err(_) => 30,
            };

        let read_rate_limit_per_minute =
            match env::var("READ_RATE_LIMIT_PER_MINUTE") {
                Ok(value) => value.parse::<u32>().map_err(|_| {
                    format!(
                        "READ_RATE_LIMIT_PER_MINUTE must be a valid u32 integer, got '{value}'"
                    )
                })?,
                Err(_) => 1000,
            };

        let profile_api_base_url = env::var("PROFILE_API_BASE_URL")
            .unwrap_or_else(|_| format!("http://127.0.0.1:{port}"));

        if profile_api_base_url.trim().is_empty() {
            return Err("PROFILE_API_BASE_URL cannot be empty".to_string());
        }

        let post_content_api_base_url = env::var("POST_CONTENT_API_BASE_URL")
            .unwrap_or_else(|_| format!("http://127.0.0.1:{port}"));
        let bonus_hunter_content_api_base_url = env::var("BONUS_HUNTER_CONTENT_API_BASE_URL")
            .unwrap_or_else(|_| format!("http://127.0.0.1:{port}"));
        let top_picks_content_api_base_url = env::var("TOP_PICKS_CONTENT_API_BASE_URL")
            .unwrap_or_else(|_| format!("http://127.0.0.1:{port}"));

        if post_content_api_base_url.trim().is_empty() {
            return Err("POST_CONTENT_API_BASE_URL cannot be empty".to_string());
        }

        if bonus_hunter_content_api_base_url.trim().is_empty() {
            return Err("BONUS_HUNTER_CONTENT_API_BASE_URL cannot be empty".to_string());
        }

        if top_picks_content_api_base_url.trim().is_empty() {
            return Err("TOP_PICKS_CONTENT_API_BASE_URL cannot be empty".to_string());
        }

        Ok(Self {
            host,
            port,
            database_url,
            redis_url,
            write_rate_limit_per_minute,
            read_rate_limit_per_minute,
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
