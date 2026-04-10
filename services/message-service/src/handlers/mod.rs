use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use uuid::Uuid;

use crate::db;
use crate::models::{
    CreateMessageRequest, ListMessagesQuery, MarkSyncFailedRequest, MarkSyncedRequest,
    MessageResponse,
};
use crate::AppState;

/// Health check endpoint
pub async fn health_check() -> impl IntoResponse {
    Json(serde_json::json!({ "status": "healthy", "service": "message-service" }))
}

/// Create a new message
///
/// POST /api/messages
pub async fn create_message(
    State(state): State<AppState>,
    Json(payload): Json<CreateMessageRequest>,
) -> impl IntoResponse {
    // Validate customer exists via Customer Service
    match state
        .customer_client
        .customer_exists(payload.customer_id)
        .await
    {
        Ok(false) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": "Customer not found",
                    "customer_id": payload.customer_id.to_string()
                })),
            )
                .into_response();
        }
        Err(e) => {
            tracing::warn!("Failed to validate customer {}: {}", payload.customer_id, e);
            // Continue anyway - Customer Service might be temporarily unavailable
        }
        Ok(true) => {}
    }

    // Check for duplicate external_id if provided
    if let Some(ref external_id) = payload.external_id {
        match db::get_message_by_external_id(&state.pool, external_id).await {
            Ok(Some(existing)) => {
                let response: MessageResponse = existing.into();
                return (StatusCode::OK, Json(response)).into_response();
            }
            Ok(None) => {}
            Err(e) => {
                tracing::warn!("Failed to check for duplicate external_id: {}", e);
            }
        }
    }

    match db::save_message(
        &state.pool,
        payload.customer_id,
        &payload.conversation_id,
        &payload.platform,
        &payload.direction,
        payload.message_text.as_deref(),
        payload.external_id.as_deref(),
        payload.created_at,
    )
    .await
    {
        Ok(message) => {
            let response: MessageResponse = message.into();
            (StatusCode::CREATED, Json(response)).into_response()
        }
        Err(e) => {
            tracing::error!("Failed to create message: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": "Failed to create message",
                    "details": e.to_string()
                })),
            )
                .into_response()
        }
    }
}

/// Get message by ID
///
/// GET /api/messages/:id
pub async fn get_message(State(state): State<AppState>, Path(id): Path<Uuid>) -> impl IntoResponse {
    match db::get_message_by_id(&state.pool, id).await {
        Ok(Some(message)) => {
            let response: MessageResponse = message.into();
            (StatusCode::OK, Json(response)).into_response()
        }
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": "Message not found",
                "id": id.to_string()
            })),
        )
            .into_response(),
        Err(e) => {
            tracing::error!("Failed to get message {}: {}", id, e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": "Failed to get message",
                    "details": e.to_string()
                })),
            )
                .into_response()
        }
    }
}

/// Get messages by customer ID
///
/// GET /api/messages/customer/:customer_id
pub async fn get_messages_by_customer(
    State(state): State<AppState>,
    Path(customer_id): Path<Uuid>,
    Query(query): Query<ListMessagesQuery>,
) -> impl IntoResponse {
    match db::get_messages_by_customer_id(&state.pool, customer_id, query.limit, query.offset).await
    {
        Ok(messages) => {
            let responses: Vec<MessageResponse> = messages.into_iter().map(|m| m.into()).collect();
            (StatusCode::OK, Json(responses)).into_response()
        }
        Err(e) => {
            tracing::error!("Failed to get messages for customer {}: {}", customer_id, e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": "Failed to get messages",
                    "details": e.to_string()
                })),
            )
                .into_response()
        }
    }
}

/// Get messages by conversation ID
///
/// GET /api/messages/conversation/:conversation_id
pub async fn get_messages_by_conversation(
    State(state): State<AppState>,
    Path(conversation_id): Path<String>,
    Query(query): Query<ListMessagesQuery>,
) -> impl IntoResponse {
    match db::get_messages_by_conversation_id(
        &state.pool,
        &conversation_id,
        query.limit,
        query.offset,
    )
    .await
    {
        Ok(messages) => {
            let responses: Vec<MessageResponse> = messages.into_iter().map(|m| m.into()).collect();
            (StatusCode::OK, Json(responses)).into_response()
        }
        Err(e) => {
            tracing::error!(
                "Failed to get messages for conversation {}: {}",
                conversation_id,
                e
            );
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": "Failed to get messages",
                    "details": e.to_string()
                })),
            )
                .into_response()
        }
    }
}

/// Get unsynced messages for Mattermost sync
///
/// GET /api/messages/unsynced
pub async fn get_unsynced_messages(State(state): State<AppState>) -> impl IntoResponse {
    match db::get_unsynced_messages(&state.pool, 100).await {
        Ok(messages) => {
            let responses: Vec<MessageResponse> = messages.into_iter().map(|m| m.into()).collect();
            (StatusCode::OK, Json(responses)).into_response()
        }
        Err(e) => {
            tracing::error!("Failed to get unsynced messages: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": "Failed to get unsynced messages",
                    "details": e.to_string()
                })),
            )
                .into_response()
        }
    }
}

/// Mark message as synced to Mattermost
///
/// PUT /api/messages/:id/synced
pub async fn mark_message_synced(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(payload): Json<MarkSyncedRequest>,
) -> impl IntoResponse {
    match db::mark_message_synced(&state.pool, id, &payload.mattermost_channel).await {
        Ok(Some(message)) => {
            let response: MessageResponse = message.into();
            (StatusCode::OK, Json(response)).into_response()
        }
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": "Message not found",
                "id": id.to_string()
            })),
        )
            .into_response(),
        Err(e) => {
            tracing::error!("Failed to mark message {} as synced: {}", id, e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": "Failed to mark message as synced",
                    "details": e.to_string()
                })),
            )
                .into_response()
        }
    }
}

/// Mark message sync as failed
///
/// PUT /api/messages/:id/sync-failed
pub async fn mark_message_sync_failed(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(payload): Json<MarkSyncFailedRequest>,
) -> impl IntoResponse {
    match db::mark_message_sync_failed(&state.pool, id, &payload.error).await {
        Ok(Some(message)) => {
            let response: MessageResponse = message.into();
            (StatusCode::OK, Json(response)).into_response()
        }
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": "Message not found",
                "id": id.to_string()
            })),
        )
            .into_response(),
        Err(e) => {
            tracing::error!("Failed to mark message {} sync as failed: {}", id, e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": "Failed to mark message sync as failed",
                    "details": e.to_string()
                })),
            )
                .into_response()
        }
    }
}

/// Lookup customer UUID by conversation ID (for bridge bot).
pub async fn lookup_customer_by_conversation(
    State(state): State<AppState>,
    axum::extract::Path(conversation_id): axum::extract::Path<String>,
) -> impl IntoResponse {
    match db::get_customer_id_by_conversation(&state.pool, &conversation_id).await {
        Ok(Some(customer_id)) => (
            StatusCode::OK,
            Json(serde_json::json!({ "customer_id": customer_id })),
        )
            .into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": "No messages found for conversation",
                "conversation_id": conversation_id
            })),
        )
            .into_response(),
        Err(e) => {
            tracing::error!(
                "Failed to lookup customer for conv {}: {}",
                conversation_id,
                e
            );
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": "Database error",
                    "details": e.to_string()
                })),
            )
                .into_response()
        }
    }
}
