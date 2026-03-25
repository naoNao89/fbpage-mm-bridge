//! Customer Service
//!
//! A microservice for managing customer identity, profiles, and platform mappings.
//!
//! ## API Endpoints
//!
//! - `GET /health` - Health check
//! - `GET /api/customers/:id` - Get customer by ID
//! - `GET /api/customers/platform/:platform/:user_id` - Get customer by platform ID
//! - `POST /api/customers` - Create or get customer (idempotent)
//! - `PUT /api/customers/:id` - Update customer profile
//! - `GET /api/customers` - List customers with optional filtering
//! - `GET /api/customers/without-mapping` - Get customers without channel mappings
//! - `GET /api/customers/stats` - Get customer statistics

pub mod config;
pub mod db;
pub mod handlers;
pub mod models;

use axum::{
    routing::{get, post, put},
    Router,
};
use sqlx::PgPool;

use crate::config::Config;
use crate::handlers::{
    create_or_get_customer, get_customer, get_customer_by_platform, get_customer_stats,
    get_customers_without_mapping, health_check, list_customers, update_customer,
};

/// Application state shared across handlers
#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub config: Config,
}

/// Create the application router
pub fn create_app(state: AppState) -> Router {
    Router::new()
        // Health check
        .route("/health", get(health_check))
        // Customer CRUD operations
        .route("/api/customers", post(create_or_get_customer))
        .route("/api/customers", get(list_customers))
        .route("/api/customers/stats", get(get_customer_stats))
        .route(
            "/api/customers/without-mapping",
            get(get_customers_without_mapping),
        )
        .route("/api/customers/:id", get(get_customer))
        .route("/api/customers/:id", put(update_customer))
        .route(
            "/api/customers/platform/:platform/:user_id",
            get(get_customer_by_platform),
        )
        .with_state(state)
}

/// Run database migrations
pub async fn run_migrations(pool: &PgPool) -> anyhow::Result<()> {
    sqlx::migrate!("./migrations").run(pool).await?;
    Ok(())
}
