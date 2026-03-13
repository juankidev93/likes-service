use std::env;

pub struct ServiceConfig {
    pub host: String,
    pub port: u16,
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

        Ok(Self { host, port })
    }

    pub fn bind_address(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}
