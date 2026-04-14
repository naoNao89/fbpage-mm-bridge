use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

// ============================================================================
// Database Models
// ============================================================================

/// Rate limit tracking from Facebook API
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct FacebookRateLimit {
    pub id: Uuid,
    pub endpoint: String,
    pub calls_remaining: Option<i32>,
    pub calls_total: Option<i32>,
    pub reset_at: Option<DateTime<Utc>>,
    pub last_response_headers: Option<serde_json::Value>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Import job tracking
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ImportJob {
    pub id: Uuid,
    pub status: String,
    pub total_conversations: Option<i32>,
    pub processed_conversations: Option<i32>,
    pub failed_conversations: Option<i32>,
    pub total_messages: Option<i32>,
    pub messages_stored: Option<i32>,
    pub messages_skipped: Option<i32>,
    pub error_message: Option<String>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Conversation import tracking
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ConversationImport {
    pub id: Uuid,
    pub job_id: Option<Uuid>,
    pub conversation_id: String,
    pub status: String,
    pub messages_fetched: Option<i32>,
    pub messages_stored: Option<i32>,
    pub error_message: Option<String>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

// ============================================================================
// Facebook Graph API Models
// ============================================================================

/// Facebook conversation from Graph API
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conversation {
    pub id: String,
    pub updated_time: DateTime<Utc>,
    #[serde(default)]
    pub message_count: Option<i32>,
}

/// Facebook conversations response with paging
#[derive(Debug, Deserialize)]
pub struct ConversationsResponse {
    pub data: Vec<Conversation>,
    pub paging: Option<Paging>,
}

/// Paging information for cursor-based pagination
#[derive(Debug, Deserialize)]
pub struct Paging {
    pub cursors: Option<Cursors>,
    pub next: Option<String>,
}

/// Cursor positions for pagination
#[derive(Debug, Deserialize)]
pub struct Cursors {
    pub before: String,
    pub after: String,
}

/// Facebook message from Graph API
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphMessage {
    pub id: String,
    pub created_time: DateTime<Utc>,
    pub from: GraphParticipant,
    pub message: Option<String>,
    pub to: GraphTo,
    #[serde(default)]
    pub attachments: Option<GraphAttachmentsResponse>,
}

/// Participant in a Facebook conversation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphParticipant {
    pub name: String,
    pub id: String,
    #[serde(default)]
    pub email: Option<String>,
}

/// Recipients of a message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphTo {
    pub data: Vec<GraphParticipant>,
}

/// Attachments response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphAttachmentsResponse {
    pub data: Vec<GraphAttachment>,
}

/// A single attachment
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphAttachment {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub mime_type: Option<String>,
    #[serde(default)]
    pub size: Option<i64>,
    #[serde(default)]
    pub image_data: Option<GraphAttachmentData>,
    #[serde(default)]
    pub video_data: Option<GraphAttachmentData>,
    #[serde(default)]
    pub file_url: Option<String>,
}

/// Attachment data (image/video)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphAttachmentData {
    pub url: String,
    #[serde(default)]
    pub width: Option<i32>,
    #[serde(default)]
    pub height: Option<i32>,
    #[serde(default)]
    pub preview_url: Option<String>,
}

/// Messages response with paging
#[derive(Debug, Deserialize)]
pub struct MessagesResponse {
    pub data: Vec<GraphMessage>,
    pub paging: Option<Paging>,
}

/// Conversation import status tracking
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationImportStatus {
    pub conversation_id: String,
    pub status: String,
    pub messages_fetched: i32,
    pub messages_stored: i32,
    pub error: Option<String>,
}

impl ConversationImportStatus {
    pub fn pending() -> Self {
        Self {
            conversation_id: String::new(),
            status: "pending".to_string(),
            messages_fetched: 0,
            messages_stored: 0,
            error: None,
        }
    }
}

/// Rate limit info extracted from Facebook headers
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FacebookRateLimitInfo {
    pub call_count: Option<i32>,
    pub total_cputime: Option<i32>,
    pub total_time: Option<i32>,
    pub call_count_limit: Option<i32>,
    pub cputime_limit: Option<i32>,
    pub time_limit: Option<i32>,
}

// ============================================================================
// API Request/Response Models
// ============================================================================

/// Response for import operations
#[derive(Debug, Serialize)]
pub struct ImportResponse {
    pub status: String,
    pub job_id: Uuid,
    pub message: String,
}

/// Response for import status
#[derive(Debug, Serialize)]
pub struct ImportStatusResponse {
    pub status: String,
    pub total_conversations: i32,
    pub processed_conversations: i32,
    pub failed_conversations: i32,
    pub total_messages: i32,
    pub messages_stored: i32,
    pub messages_skipped: i32,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
}

/// Conversation import result
#[derive(Debug, Serialize)]
pub struct ConversationImportResult {
    pub conversation_id: String,
    pub status: String,
    pub messages_fetched: i32,
    pub messages_stored: i32,
    pub messages_skipped: i32,
    pub error: Option<String>,
}
