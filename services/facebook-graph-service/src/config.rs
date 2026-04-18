use serde::Deserialize;
use std::env;

/// Application configuration
#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    /// Server bind address
    pub bind_address: String,
    /// Log level (e.g., "info", "debug")
    pub log_level: String,
    /// Database connection URL
    pub database_url: String,
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
    /// Rate limit warning threshold (percentage)
    #[serde(default = "default_rate_limit_warning_threshold")]
    pub rate_limit_warning_threshold: f32,
    /// Rate limit critical threshold (percentage)
    #[serde(default = "default_rate_limit_critical_threshold")]
    pub rate_limit_critical_threshold: f32,
    /// Polling interval in seconds for real-time message detection (0 = disabled)
    #[serde(default = "default_poll_interval")]
    pub poll_interval_secs: u64,
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

impl Config {
    /// Load configuration from environment variables
    pub fn from_env() -> anyhow::Result<Self> {
        Ok(Config {
            bind_address: env::var("BIND_ADDRESS").unwrap_or_else(|_| "0.0.0.0:3003".to_string()),
            log_level: env::var("LOG_LEVEL").unwrap_or_else(|_| "info".to_string()),
            database_url: env::var("DATABASE_URL").context("DATABASE_URL must be set")?,
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
        })
    }
}

use std::fmt;
impl fmt::Display for Config {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Config {{ bind_address: {}, facebook_page_id: {}, customer_service_url: {}, message_service_url: {} }}",
            self.bind_address,
            self.facebook_page_id,
            self.customer_service_url,
            self.message_service_url
        )
    }
}

use anyhow::Context;
