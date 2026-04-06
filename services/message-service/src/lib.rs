//! Message Service
//!
//! A microservice for storing and managing messages from various platforms (Facebook, etc.).
//!
//! ## API Endpoints
//!
//! - `GET /health` - Health check
//! - `POST /api/messages` - Store new message
//! - `GET /api/messages/:id` - Get message by ID
//! - `GET /api/messages/customer/:customer_id` - Get messages by customer ID
//! - `GET /api/messages/conversation/:conversation_id` - Get messages by conversation ID
//! - `GET /api/messages/unsynced` - Get unsynced messages
//! - `PUT /api/messages/:id/synced` - Mark message as synced to Mattermost
//! - `PUT /api/messages/:id/sync-failed` - Mark message sync as failed

pub mod config;
pub mod db;
pub mod handlers;
pub mod models;
pub mod services;

use axum::{
    routing::{get, post, put},
    Router,
};
use sqlx::PgPool;

use crate::config::Config;
use crate::handlers::{
    create_message, get_message, get_messages_by_conversation, get_messages_by_customer,
    get_unsynced_messages, health_check, lookup_customer_by_conversation,
    mark_message_sync_failed, mark_message_synced,
};
use crate::services::customer_client::CustomerServiceClient;

/// Application state shared across handlers
#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub config: Config,
    pub customer_client: CustomerServiceClient,
}

/// Create the application router
pub fn create_app(state: AppState) -> Router {
    Router::new()
        // Health check
        .route("/health", get(health_check))
        // Message operations
        .route("/api/messages", post(create_message))
        .route("/api/messages/unsynced", get(get_unsynced_messages))
        .route("/api/messages/:id", get(get_message))
        .route("/api/messages/:id/synced", put(mark_message_synced))
        .route(
            "/api/messages/:id/sync-failed",
            put(mark_message_sync_failed),
        )
        .route(
            "/api/messages/customer/:customer_id",
            get(get_messages_by_customer),
        )
        .route(
            "/api/messages/conversation/:conversation_id",
            get(get_messages_by_conversation),
        )
        .route(
            "/api/messages/conversation/:conversation_id/customer",
            get(lookup_customer_by_conversation),
        )
        .with_state(state)
}

/// Run database migrations
pub async fn run_migrations(pool: &PgPool) -> anyhow::Result<()> {
    sqlx::migrate!("./migrations").run(pool).await?;
    Ok(())
}
