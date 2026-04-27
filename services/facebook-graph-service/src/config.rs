use serde::Deserialize;
use shared_utils::env_u32;
use std::env;
use std::str::FromStr;

/// Controls whether Mattermost direct-DB-bypass operations are available.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BypassMode {
    Off,
    Shadow,
    Enabled,
}

impl BypassMode {
    pub fn as_str(self) -> &'static str {
        match self {
            BypassMode::Off => "off",
            BypassMode::Shadow => "shadow",
            BypassMode::Enabled => "enabled",
        }
    }
}

impl FromStr for BypassMode {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.to_ascii_lowercase().as_str() {
            "off" => Ok(BypassMode::Off),
            "shadow" => Ok(BypassMode::Shadow),
            "enabled" => Ok(BypassMode::Enabled),
            other => Err(anyhow::anyhow!(
                "invalid MATTERMOST_BYPASS_MODE {other:?}; expected off|shadow|enabled"
            )),
        }
    }
}

/// Application configuration
#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    /// Server bind address
    pub bind_address: String,
    /// Log level (e.g., "info", "debug")
    pub log_level: String,
    /// Database connection URL
    pub database_url: String,
    pub database_max_connections: u32,
    /// Facebook Page ID
    pub facebook_page_id: String,
    /// Facebook Page Access Token
    pub facebook_page_access_token: String,
    /// Facebook App ID (for token exchange)
    pub facebook_app_id: String,
    /// Facebook App Secret (for token exchange)
    pub facebook_app_secret: String,
    /// Facebook Webhook Verify Token (for Facebook webhook verification)
    pub facebook_webhook_verify_token: String,
    /// Instagram Business Account ID
    pub instagram_ig_user_id: String,
    /// Instagram Webhook Verify Token (for Instagram webhook verification)
    pub instagram_webhook_verify_token: String,
    /// Customer Service URL
    pub customer_service_url: String,
    /// Message Service URL
    pub message_service_url: String,
    /// Mattermost REST API URL
    pub mattermost_url: String,
    /// Mattermost admin username
    pub mattermost_username: String,
    pub mattermost_password: Option<String>,
    /// Mattermost database URL (for direct DB access bypassing API)
    pub mattermost_database_url: Option<String>,
    pub mattermost_database_max_connections: u32,
    /// Mattermost DB-bypass mode (`off`, `shadow`, `enabled`)
    pub mattermost_bypass_mode: BypassMode,
    /// Bearer token required for `/api/mm-admin/*`
    pub mm_admin_api_token: Option<String>,
    /// Rate limit warning threshold (percentage)
    #[serde(default = "default_rate_limit_warning_threshold")]
    pub rate_limit_warning_threshold: f32,
    /// Rate limit critical threshold (percentage)
    #[serde(default = "default_rate_limit_critical_threshold")]
    pub rate_limit_critical_threshold: f32,
    /// Polling interval in seconds for real-time message detection (0 = disabled)
    #[serde(default = "default_poll_interval")]
    pub poll_interval_secs: u64,
    /// MinIO endpoint URL (e.g., "http://minio:9000")
    #[serde(default = "default_minio_endpoint")]
    pub minio_endpoint: String,
    /// MinIO access key
    #[serde(default = "default_minio_access_key")]
    pub minio_access_key: String,
    /// MinIO secret key
    #[serde(default = "default_minio_secret_key")]
    pub minio_secret_key: String,
    /// MinIO bucket name for media storage
    #[serde(default = "default_minio_bucket")]
    pub minio_bucket: String,
    /// MinIO presigned URL TTL in seconds
    #[serde(default = "default_minio_presigned_ttl")]
    pub minio_presigned_ttl_secs: u64,
}

fn default_rate_limit_warning_threshold() -> f32 {
    80.0
}

fn default_rate_limit_critical_threshold() -> f32 {
    95.0
}

fn default_poll_interval() -> u64 {
    30
}

fn default_minio_endpoint() -> String {
    "http://minio:9000".to_string()
}

fn default_minio_access_key() -> String {
    "minioadmin".to_string()
}

fn default_minio_secret_key() -> String {
    "minioadmin".to_string()
}

fn default_minio_bucket() -> String {
    "fb-mm-media".to_string()
}

fn default_minio_presigned_ttl() -> u64 {
    86400
}

impl Config {
    /// Load configuration from environment variables
    pub fn from_env() -> anyhow::Result<Self> {
        Ok(Config {
            bind_address: env::var("BIND_ADDRESS").unwrap_or_else(|_| "0.0.0.0:3003".to_string()),
            log_level: env::var("LOG_LEVEL").unwrap_or_else(|_| "info".to_string()),
            database_url: env::var("DATABASE_URL").context("DATABASE_URL must be set")?,
            database_max_connections: env_u32("DATABASE_MAX_CONNECTIONS", 10),
            facebook_page_id: env::var("FACEBOOK_PAGE_ID")
                .context("FACEBOOK_PAGE_ID must be set")?,
            facebook_page_access_token: env::var("FACEBOOK_PAGE_ACCESS_TOKEN")
                .context("FACEBOOK_PAGE_ACCESS_TOKEN must be set")?,
            facebook_app_id: env::var("FACEBOOK_APP_ID").unwrap_or_default(),
            facebook_app_secret: env::var("FACEBOOK_APP_SECRET").unwrap_or_default(),
            facebook_webhook_verify_token: env::var("FACEBOOK_WEBHOOK_VERIFY_TOKEN")
                .unwrap_or_default(),
            instagram_ig_user_id: env::var("INSTAGRAM_IG_USER_ID").unwrap_or_default(),
            instagram_webhook_verify_token: env::var("INSTAGRAM_WEBHOOK_VERIFY_TOKEN")
                .unwrap_or_default(),
            customer_service_url: env::var("CUSTOMER_SERVICE_URL")
                .context("CUSTOMER_SERVICE_URL must be set")?,
            message_service_url: env::var("MESSAGE_SERVICE_URL")
                .context("MESSAGE_SERVICE_URL must be set")?,
            mattermost_url: env::var("MATTERMOST_URL")
                .unwrap_or_else(|_| "http://localhost:8065".to_string()),
            mattermost_username: env::var("MATTERMOST_USERNAME")
                .unwrap_or_else(|_| "admin".to_string()),
            mattermost_password: env::var("MATTERMOST_PASSWORD").ok(),
            mattermost_database_url: env::var("MATTERMOST_DATABASE_URL").ok(),
            mattermost_database_max_connections: env_u32("MATTERMOST_DATABASE_MAX_CONNECTIONS", 5),
            mattermost_bypass_mode: env::var("MATTERMOST_BYPASS_MODE")
                .unwrap_or_else(|_| "off".to_string())
                .parse()
                .unwrap_or(BypassMode::Off),
            mm_admin_api_token: env::var("MM_ADMIN_API_TOKEN").ok(),
            rate_limit_warning_threshold: env::var("RATE_LIMIT_WARNING_THRESHOLD")
                .unwrap_or_else(|_| "80.0".to_string())
                .parse()
                .unwrap_or(80.0),
            rate_limit_critical_threshold: env::var("RATE_LIMIT_CRITICAL_THRESHOLD")
                .unwrap_or_else(|_| "95.0".to_string())
                .parse()
                .unwrap_or(95.0),
            poll_interval_secs: env::var("POLL_INTERVAL_SECS")
                .unwrap_or_else(|_| "30".to_string())
                .parse()
                .unwrap_or(30),
            minio_endpoint: env::var("MINIO_ENDPOINT")
                .unwrap_or_else(|_| "http://minio:9000".to_string()),
            minio_access_key: env::var("MINIO_ACCESS_KEY")
                .unwrap_or_else(|_| "minioadmin".to_string()),
            minio_secret_key: env::var("MINIO_SECRET_KEY")
                .unwrap_or_else(|_| "minioadmin".to_string()),
            minio_bucket: env::var("MINIO_BUCKET").unwrap_or_else(|_| "fb-mm-media".to_string()),
            minio_presigned_ttl_secs: env::var("MINIO_PRESIGNED_TTL_SECS")
                .unwrap_or_else(|_| "86400".to_string())
                .parse()
                .unwrap_or(86400),
        })
    }
}

use std::fmt;
impl fmt::Display for Config {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Config {{ bind_address: {}, facebook_page_id: {}, customer_service_url: {}, message_service_url: {}, minio_endpoint: {}, minio_bucket: {} }}",
            self.bind_address,
            self.facebook_page_id,
            self.customer_service_url,
            self.message_service_url,
            self.minio_endpoint,
            self.minio_bucket
        )
    }
}

use anyhow::Context;

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::sync::{Mutex, OnceLock};

    static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    fn with_env_lock<T>(test: impl FnOnce() -> T) -> T {
        let _guard = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        test()
    }

    fn clear_env() {
        for key in [
            "BIND_ADDRESS",
            "LOG_LEVEL",
            "DATABASE_URL",
            "DATABASE_MAX_CONNECTIONS",
            "FACEBOOK_PAGE_ID",
            "FACEBOOK_PAGE_ACCESS_TOKEN",
            "FACEBOOK_APP_ID",
            "FACEBOOK_APP_SECRET",
            "FACEBOOK_WEBHOOK_VERIFY_TOKEN",
            "INSTAGRAM_IG_USER_ID",
            "INSTAGRAM_WEBHOOK_VERIFY_TOKEN",
            "CUSTOMER_SERVICE_URL",
            "MESSAGE_SERVICE_URL",
            "MATTERMOST_URL",
            "MATTERMOST_USERNAME",
            "MATTERMOST_PASSWORD",
            "MATTERMOST_DATABASE_URL",
            "MATTERMOST_DATABASE_MAX_CONNECTIONS",
            "MATTERMOST_BYPASS_MODE",
            "MM_ADMIN_API_TOKEN",
            "RATE_LIMIT_WARNING_THRESHOLD",
            "RATE_LIMIT_CRITICAL_THRESHOLD",
            "POLL_INTERVAL_SECS",
            "MINIO_ENDPOINT",
            "MINIO_ACCESS_KEY",
            "MINIO_SECRET_KEY",
            "MINIO_BUCKET",
            "MINIO_PRESIGNED_TTL_SECS",
        ] {
            env::remove_var(key);
        }
    }

    fn set_required_env() {
        env::set_var("DATABASE_URL", "postgres://facebook");
        env::set_var("FACEBOOK_PAGE_ID", "page-id");
        env::set_var("FACEBOOK_PAGE_ACCESS_TOKEN", "page-token");
        env::set_var("CUSTOMER_SERVICE_URL", "http://customer:3001");
        env::set_var("MESSAGE_SERVICE_URL", "http://message:3002");
    }

    #[test]
    fn from_env_uses_database_pool_defaults() {
        with_env_lock(|| {
            clear_env();
            set_required_env();

            let config = Config::from_env().unwrap();

            clear_env();
            assert_eq!(config.database_max_connections, 10);
            assert_eq!(config.mattermost_database_max_connections, 5);
            assert_eq!(config.mattermost_bypass_mode, BypassMode::Off);
            assert_eq!(config.mattermost_url, "http://localhost:8065");
            assert_eq!(config.mattermost_username, "admin");
        });
    }

    #[test]
    fn from_env_reads_database_pool_overrides() {
        with_env_lock(|| {
            clear_env();
            set_required_env();
            env::set_var("DATABASE_MAX_CONNECTIONS", "31");
            env::set_var("MATTERMOST_DATABASE_MAX_CONNECTIONS", "7");

            let config = Config::from_env().unwrap();

            clear_env();
            assert_eq!(config.database_max_connections, 31);
            assert_eq!(config.mattermost_database_max_connections, 7);
        });
    }

    #[test]
    fn from_env_defaults_invalid_database_pool_overrides() {
        with_env_lock(|| {
            clear_env();
            set_required_env();
            env::set_var("DATABASE_MAX_CONNECTIONS", "invalid");
            env::set_var("MATTERMOST_DATABASE_MAX_CONNECTIONS", "invalid");

            let config = Config::from_env().unwrap();

            clear_env();
            assert_eq!(config.database_max_connections, 10);
            assert_eq!(config.mattermost_database_max_connections, 5);
        });
    }
}
