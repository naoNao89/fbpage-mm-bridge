use anyhow::Result;
use std::env;

/// Configuration for the Webhook Gateway
#[derive(Debug, Clone)]
pub struct Config {
    /// Server bind address (e.g., "0.0.0.0:3004")
    pub bind_address: String,
    /// Log level (e.g., "info", "debug")
    pub log_level: String,
    /// Facebook webhook verify token
    pub facebook_verify_token: String,
    /// Mattermost incoming webhook URL
    pub mattermost_webhook_url: String,
}

impl Config {
    /// Load configuration from environment variables
    pub fn from_env() -> Result<Self> {
        let bind_address = env::var("BIND_ADDRESS")
            .or_else(|_| env::var("WEBHOOK_GATEWAY_BIND_ADDRESS"))
            .unwrap_or_else(|_| "0.0.0.0:3004".to_string());

        let log_level = env::var("LOG_LEVEL")
            .or_else(|_| env::var("WEBHOOK_GATEWAY_LOG_LEVEL"))
            .unwrap_or_else(|_| "info".to_string());

        let facebook_verify_token =
            env::var("FACEBOOK_VERIFY_TOKEN").expect("FACEBOOK_VERIFY_TOKEN must be set");

        let mattermost_webhook_url =
            env::var("MATTERMOST_WEBHOOK_URL").expect("MATTERMOST_WEBHOOK_URL must be set");

        Ok(Self {
            bind_address,
            log_level,
            facebook_verify_token,
            mattermost_webhook_url,
        })
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            bind_address: "0.0.0.0:3004".to_string(),
            log_level: "info".to_string(),
            facebook_verify_token: "your-verify-token-here".to_string(),
            mattermost_webhook_url: "http://localhost:8065/hooks/test".to_string(),
        }
    }
}
