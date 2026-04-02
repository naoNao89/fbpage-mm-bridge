//! Test utilities for Customer Service integration tests
//!
//! This module provides common utilities for setting up test databases,
//! creating test fixtures, and managing test transactions.

use sqlx::PgPool;

/// Create a test database connection pool
///
/// Uses TEST_DATABASE_URL environment variable if set, otherwise falls back to DATABASE_URL.
/// The pool is configured with a maximum of 5 connections for test isolation.
pub async fn setup_test_db() -> PgPool {
    dotenvy::dotenv().ok();

    let database_url = env::var("TEST_DATABASE_URL")
        .or_else(|_| env::var("DATABASE_URL"))
        .expect("TEST_DATABASE_URL or DATABASE_URL must be set for tests");

    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&database_url)
        .await
        .expect("Failed to connect to test database");

    // Run migrations
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("Failed to run migrations");

    pool
}

/// Clean up test database by truncating all tables
///
/// This is used to ensure test isolation between tests.
/// Tables are truncated in CASCADE mode to handle foreign key constraints.
pub async fn cleanup_test_db(pool: &PgPool) {
    sqlx::query("TRUNCATE TABLE customers CASCADE")
        .execute(pool)
        .await
        .expect("Failed to cleanup test database");
}
