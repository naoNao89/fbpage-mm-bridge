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
pub mod media;
pub mod media_worker;
pub mod models;
pub mod poll;
pub mod services;
pub mod storage;

use axum::{
    routing::{get, post},
    Router,
};

use crate::config::Config;
use crate::handlers::{
    exchange_token, get_import_status, health_check, import_all_conversations,
    import_single_conversation, reimport_all_conversations, reimport_conversation, webhook_handler,
    webhook_verification,
};
use crate::services::{CustomerServiceClient, MattermostClient, MessageServiceClient};
use crate::storage::MinioStorage;

/// Application state shared across handlers
#[derive(Clone)]
pub struct AppState {
    pub pool: sqlx::PgPool,
    pub config: Config,
    pub customer_client: CustomerServiceClient,
    pub message_client: MessageServiceClient,
    pub mattermost_client: MattermostClient,
    pub minio: Option<MinioStorage>,
}

/// Create the application router
pub fn create_app(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health_check))
        .route(
            "/webhook/facebook",
            get(webhook_verification).post(webhook_handler),
        )
        .route("/api/import/conversations", post(import_all_conversations))
        .route(
            "/api/import/conversation/:id",
            post(import_single_conversation),
        )
        .route("/api/reimport/:id", post(reimport_conversation))
        .route("/api/reimport", post(reimport_all_conversations))
        .route("/api/status", get(get_import_status))
        .route("/api/token/exchange", post(exchange_token))
        .with_state(state)
}

/// Run database migrations
pub async fn run_migrations(pool: &sqlx::PgPool) -> anyhow::Result<()> {
    sqlx::migrate!("./migrations").run(pool).await?;
    Ok(())
}
