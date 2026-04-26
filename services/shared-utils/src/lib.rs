use anyhow::Context;
use sqlx::{postgres::PgPoolOptions, PgPool};
use std::env;

pub fn env_var_with_fallback(primary: &str, fallback: &str) -> anyhow::Result<String> {
    env::var(primary)
        .or_else(|_| env::var(fallback))
        .with_context(|| format!("{primary} or {fallback} must be set"))
}

pub fn optional_env_var_with_fallback(primary: &str, fallback: &str, default: &str) -> String {
    env::var(primary)
        .or_else(|_| env::var(fallback))
        .unwrap_or_else(|_| default.to_string())
}

pub fn env_u32_with_fallback(primary: &str, fallback: &str, default: u32) -> u32 {
    env::var(primary)
        .or_else(|_| env::var(fallback))
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

pub fn env_u32(primary: &str, default: u32) -> u32 {
    env::var(primary)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

pub async fn create_pg_pool(database_url: &str, max_connections: u32) -> anyhow::Result<PgPool> {
    PgPoolOptions::new()
        .max_connections(max_connections)
        .connect(database_url)
        .await
        .context("Failed to connect to database")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    fn with_env_lock<T>(test: impl FnOnce() -> T) -> T {
        let _guard = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        test()
    }

    #[test]
    fn env_u32_prefers_primary_over_fallback() {
        with_env_lock(|| {
            env::set_var("SHARED_UTILS_TEST_PRIMARY", "12");
            env::set_var("SHARED_UTILS_TEST_FALLBACK", "34");

            let value = env_u32_with_fallback(
                "SHARED_UTILS_TEST_PRIMARY",
                "SHARED_UTILS_TEST_FALLBACK",
                56,
            );

            env::remove_var("SHARED_UTILS_TEST_PRIMARY");
            env::remove_var("SHARED_UTILS_TEST_FALLBACK");

            assert_eq!(value, 12);
        });
    }

    #[test]
    fn env_u32_uses_fallback_when_primary_missing() {
        with_env_lock(|| {
            env::remove_var("SHARED_UTILS_TEST_PRIMARY");
            env::set_var("SHARED_UTILS_TEST_FALLBACK", "34");

            let value = env_u32_with_fallback(
                "SHARED_UTILS_TEST_PRIMARY",
                "SHARED_UTILS_TEST_FALLBACK",
                56,
            );

            env::remove_var("SHARED_UTILS_TEST_FALLBACK");

            assert_eq!(value, 34);
        });
    }

    #[test]
    fn env_u32_uses_default_for_invalid_values() {
        with_env_lock(|| {
            env::set_var("SHARED_UTILS_TEST_PRIMARY", "invalid");
            env::remove_var("SHARED_UTILS_TEST_FALLBACK");

            let value = env_u32_with_fallback(
                "SHARED_UTILS_TEST_PRIMARY",
                "SHARED_UTILS_TEST_FALLBACK",
                56,
            );

            env::remove_var("SHARED_UTILS_TEST_PRIMARY");

            assert_eq!(value, 56);
        });
    }
}
