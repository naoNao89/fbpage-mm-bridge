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
    pub created_at: Option<DateTime<Utc>>,
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

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct MessageAttachment {
    pub id: Uuid,
    pub message_id: Uuid,
    pub attachment_type: String,
    pub external_id: Option<String>,
    pub name: Option<String>,
    pub mime_type: Option<String>,
    pub size_bytes: Option<i64>,
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub cdn_url: Option<String>,
    pub cdn_url_expires_at: Option<DateTime<Utc>>,
    pub minio_key: Option<String>,
    pub minio_bucket: Option<String>,
    pub minio_etag: Option<String>,
    pub mm_file_id: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateAttachmentRequest {
    pub message_id: Uuid,
    pub attachment_type: String,
    pub external_id: Option<String>,
    pub name: Option<String>,
    pub mime_type: Option<String>,
    pub size_bytes: Option<i64>,
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub cdn_url: Option<String>,
    pub cdn_url_expires_at: Option<DateTime<Utc>>,
    pub minio_key: Option<String>,
    pub minio_bucket: Option<String>,
    pub minio_etag: Option<String>,
    pub mm_file_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttachmentResponse {
    pub id: Uuid,
    pub message_id: Uuid,
    pub attachment_type: String,
    pub external_id: Option<String>,
    pub name: Option<String>,
    pub mime_type: Option<String>,
    pub size_bytes: Option<i64>,
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub cdn_url: Option<String>,
    pub cdn_url_expires_at: Option<DateTime<Utc>>,
    pub minio_key: Option<String>,
    pub minio_bucket: Option<String>,
    pub minio_etag: Option<String>,
    pub mm_file_id: Option<String>,
    pub created_at: DateTime<Utc>,
}

impl From<MessageAttachment> for AttachmentResponse {
    fn from(a: MessageAttachment) -> Self {
        Self {
            id: a.id,
            message_id: a.message_id,
            attachment_type: a.attachment_type,
            external_id: a.external_id,
            name: a.name,
            mime_type: a.mime_type,
            size_bytes: a.size_bytes,
            width: a.width,
            height: a.height,
            cdn_url: a.cdn_url,
            cdn_url_expires_at: a.cdn_url_expires_at,
            minio_key: a.minio_key,
            minio_bucket: a.minio_bucket,
            minio_etag: a.minio_etag,
            mm_file_id: a.mm_file_id,
            created_at: a.created_at,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateAttachmentRequest {
    pub minio_key: Option<String>,
    pub minio_bucket: Option<String>,
    pub minio_etag: Option<String>,
    pub mm_file_id: Option<String>,
    pub cdn_url_expires_at: Option<DateTime<Utc>>,
}
