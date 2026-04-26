use anyhow::Result;
use shared_utils::{env_u32_with_fallback, env_var_with_fallback, optional_env_var_with_fallback};

/// Configuration for the Customer Service
#[derive(Debug, Clone)]
pub struct Config {
    /// Database connection URL
    pub database_url: String,
    pub database_max_connections: u32,
    /// Server bind address (e.g., "0.0.0.0:3001")
    pub bind_address: String,
    /// Log level (e.g., "info", "debug")
    pub log_level: String,
}

impl Config {
    /// Load configuration from environment variables
    pub fn from_env() -> Result<Self> {
        let database_url = env_var_with_fallback("DATABASE_URL", "CUSTOMER_SERVICE_DATABASE_URL")?;

        let database_max_connections = env_u32_with_fallback(
            "DATABASE_MAX_CONNECTIONS",
            "CUSTOMER_SERVICE_DATABASE_MAX_CONNECTIONS",
            10,
        );

        let bind_address = optional_env_var_with_fallback(
            "BIND_ADDRESS",
            "CUSTOMER_SERVICE_BIND_ADDRESS",
            "0.0.0.0:3001",
        );

        let log_level =
            optional_env_var_with_fallback("LOG_LEVEL", "CUSTOMER_SERVICE_LOG_LEVEL", "info");

        Ok(Self {
            database_url,
            database_max_connections,
            bind_address,
            log_level,
        })
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            database_url: "postgresql://postgres:password@localhost:5432/customer_service"
                .to_string(),
            database_max_connections: 10,
            bind_address: "0.0.0.0:3001".to_string(),
            log_level: "info".to_string(),
        }
    }
}

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
            "DATABASE_URL",
            "CUSTOMER_SERVICE_DATABASE_URL",
            "DATABASE_MAX_CONNECTIONS",
            "CUSTOMER_SERVICE_DATABASE_MAX_CONNECTIONS",
            "BIND_ADDRESS",
            "CUSTOMER_SERVICE_BIND_ADDRESS",
            "LOG_LEVEL",
            "CUSTOMER_SERVICE_LOG_LEVEL",
        ] {
            env::remove_var(key);
        }
    }

    #[test]
    fn from_env_prefers_generic_database_pool_size() {
        with_env_lock(|| {
            clear_env();
            env::set_var("DATABASE_URL", "postgres://generic");
            env::set_var("DATABASE_MAX_CONNECTIONS", "17");
            env::set_var("CUSTOMER_SERVICE_DATABASE_MAX_CONNECTIONS", "23");

            let config = Config::from_env().unwrap();

            clear_env();
            assert_eq!(config.database_url, "postgres://generic");
            assert_eq!(config.database_max_connections, 17);
            assert_eq!(config.bind_address, "0.0.0.0:3001");
            assert_eq!(config.log_level, "info");
        });
    }

    #[test]
    fn from_env_uses_service_specific_database_fallbacks() {
        with_env_lock(|| {
            clear_env();
            env::set_var("CUSTOMER_SERVICE_DATABASE_URL", "postgres://customer");
            env::set_var("CUSTOMER_SERVICE_DATABASE_MAX_CONNECTIONS", "23");

            let config = Config::from_env().unwrap();

            clear_env();
            assert_eq!(config.database_url, "postgres://customer");
            assert_eq!(config.database_max_connections, 23);
        });
    }

    #[test]
    fn from_env_defaults_invalid_database_pool_size() {
        with_env_lock(|| {
            clear_env();
            env::set_var("DATABASE_URL", "postgres://generic");
            env::set_var("DATABASE_MAX_CONNECTIONS", "invalid");

            let config = Config::from_env().unwrap();

            clear_env();
            assert_eq!(config.database_max_connections, 10);
        });
    }
}
