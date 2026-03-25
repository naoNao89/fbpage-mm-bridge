use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

/// Message entity representing a message from a platform
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Message {
    pub id: Uuid,
    pub customer_id: Uuid,
    pub conversation_id: String,
    pub platform: String,
    pub direction: String,
    pub message_text: Option<String>,
    pub external_id: Option<String>,
    pub mattermost_channel: Option<String>,
    pub mattermost_synced_at: Option<DateTime<Utc>>,
    pub mattermost_sync_error: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// Request payload for creating a new message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateMessageRequest {
    pub customer_id: Uuid,
    pub conversation_id: String,
    pub platform: String,
    pub direction: String,
    pub message_text: Option<String>,
    pub external_id: Option<String>,
}

/// Request payload for marking message as synced
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarkSyncedRequest {
    pub mattermost_channel: String,
}

/// Request payload for marking message sync as failed
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarkSyncFailedRequest {
    pub error: String,
}

/// Response for message API endpoints
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageResponse {
    pub id: Uuid,
    pub customer_id: Uuid,
    pub conversation_id: String,
    pub platform: String,
    pub direction: String,
    pub message_text: Option<String>,
    pub external_id: Option<String>,
    pub mattermost_channel: Option<String>,
    pub mattermost_synced_at: Option<DateTime<Utc>>,
    pub mattermost_sync_error: Option<String>,
    pub created_at: DateTime<Utc>,
}

impl From<Message> for MessageResponse {
    fn from(message: Message) -> Self {
        Self {
            id: message.id,
            customer_id: message.customer_id,
            conversation_id: message.conversation_id,
            platform: message.platform,
            direction: message.direction,
            message_text: message.message_text,
            external_id: message.external_id,
            mattermost_channel: message.mattermost_channel,
            mattermost_synced_at: message.mattermost_synced_at,
            mattermost_sync_error: message.mattermost_sync_error,
            created_at: message.created_at,
        }
    }
}

/// Query parameters for listing messages
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListMessagesQuery {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

impl Default for ListMessagesQuery {
    fn default() -> Self {
        Self {
            limit: Some(50),
            offset: Some(0),
        }
    }
}

/// Statistics for messages
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageStats {
    pub total: i64,
    pub synced: i64,
    pub unsynced: i64,
    pub sync_failed: i64,
}
