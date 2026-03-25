//! Facebook Graph Service
//!
//! A microservice for fetching messages from Facebook Graph API and storing them
//! via the Message Service.
//!
//! ## API Endpoints
//!
//! - `GET /health` - Health check
//! - `POST /api/import/conversations` - Start import for all conversations
//! - `POST /api/import/conversation/:id` - Import single conversation
//! - `GET /api/status` - Get import status
//! - `POST /api/token/exchange` - Exchange short-lived token for long-lived token

pub mod config;
pub mod db;
pub mod graph_api;
pub mod handlers;
pub mod models;
pub mod services;

use axum::{
    routing::{get, post},
    Router,
};
use sqlx::PgPool;

use crate::config::Config;
use crate::handlers::{
    exchange_token, get_import_status, health_check, import_all_conversations,
    import_single_conversation,
};
use crate::services::{CustomerServiceClient, MessageServiceClient};

/// Application state shared across handlers
#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub config: Config,
    pub customer_client: CustomerServiceClient,
    pub message_client: MessageServiceClient,
}

/// Create the application router
pub fn create_app(state: AppState) -> Router {
    Router::new()
        // Health check
        .route("/health", get(health_check))
        // Import endpoints
        .route("/api/import/conversations", post(import_all_conversations))
        .route(
            "/api/import/conversation/:id",
            post(import_single_conversation),
        )
        // Status endpoint
        .route("/api/status", get(get_import_status))
        // Token exchange endpoint
        .route("/api/token/exchange", post(exchange_token))
        .with_state(state)
}

/// Run database migrations
pub async fn run_migrations(pool: &PgPool) -> anyhow::Result<()> {
    sqlx::migrate!("./migrations").run(pool).await?;
    Ok(())
}
