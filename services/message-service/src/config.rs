use anyhow::Result;
use std::env;

/// Configuration for the Message Service
#[derive(Debug, Clone)]
pub struct Config {
    /// Database connection URL
    pub database_url: String,
    /// Server bind address (e.g., "0.0.0.0:3002")
    pub bind_address: String,
    /// Log level (e.g., "info", "debug")
    pub log_level: String,
    /// Customer Service URL for validation
    pub customer_service_url: String,
}

impl Config {
    /// Load configuration from environment variables
    pub fn from_env() -> Result<Self> {
        let database_url = env::var("DATABASE_URL")
            .or_else(|_| env::var("MESSAGE_SERVICE_DATABASE_URL"))
            .expect("DATABASE_URL or MESSAGE_SERVICE_DATABASE_URL must be set");
        
        let bind_address = env::var("BIND_ADDRESS")
            .or_else(|_| env::var("MESSAGE_SERVICE_BIND_ADDRESS"))
            .unwrap_or_else(|_| "0.0.0.0:3002".to_string());
        
        let log_level = env::var("LOG_LEVEL")
            .or_else(|_| env::var("MESSAGE_SERVICE_LOG_LEVEL"))
            .unwrap_or_else(|_| "info".to_string());

        let customer_service_url = env::var("CUSTOMER_SERVICE_URL")
            .unwrap_or_else(|_| "http://localhost:3001".to_string());

        Ok(Self {
            database_url,
            bind_address,
            log_level,
            customer_service_url,
        })
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            database_url: "postgresql://postgres:password@localhost:5432/message_service".to_string(),
            bind_address: "0.0.0.0:3002".to_string(),
            log_level: "info".to_string(),
            customer_service_url: "http://localhost:3001".to_string(),
        }
    }
}
