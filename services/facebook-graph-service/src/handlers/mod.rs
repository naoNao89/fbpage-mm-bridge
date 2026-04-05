//! HTTP handlers for the Facebook Graph Service

use anyhow::Context;
use axum::{extract::State, http::StatusCode, Json};
use serde::{Deserialize, Serialize};
use std::time::Instant;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use crate::db;
use crate::graph_api;
use crate::models::{ConversationImportResult, ImportResponse, ImportStatusResponse};

/// Request body for token exchange
#[derive(Debug, Deserialize)]
pub struct TokenExchangeRequest {
    /// Short-lived user access token from Facebook Login
    pub short_lived_token: String,
}

/// Response from token exchange
#[derive(Debug, Serialize)]
pub struct TokenExchangeResponse {
    pub access_token: String,
    pub token_type: String,
    pub expires_in: i64,
}
use crate::services::MessageServicePayload;
use crate::AppState;

/// Health check handler
pub async fn health_check() -> &'static str {
    "OK"
}

// Facebook Webhook Handlers

pub async fn webhook_verification(
    State(state): State<AppState>,
    axum::extract::Query(params): axum::extract::Query<WebhookVerificationParams>,
) -> Result<String, (StatusCode, String)> {
    if params.hub_mode != "subscribe" {
        return Err((StatusCode::BAD_REQUEST, "Invalid hub.mode".to_string()));
    }
    if params.hub_verify_token != state.config.facebook_webhook_verify_token {
        return Err((StatusCode::FORBIDDEN, "Invalid verify token".to_string()));
    }
    Ok(params.hub_challenge)
}

pub async fn webhook_handler(
    State(state): State<AppState>,
    body: String,
) -> Result<StatusCode, (StatusCode, String)> {
    info!("Received webhook event");

    if let Some(entry) = parse_webhook_entry(&body) {
        for message in entry.messaging {
            let sender_id = match &message.sender.id {
                Some(id) => id,
                None => continue,
            };
            let recipient_id = match &message.recipient.id {
                Some(id) => id,
                None => continue,
            };
            let msg = match &message.message {
                Some(m) => m,
                None => continue,
            };

            let direction = if recipient_id == &state.config.facebook_page_id {
                "incoming"
            } else {
                "outgoing"
            };

            let text = msg.text.clone().or_else(|| msg.quick_reply.as_ref().map(|q| q.payload.clone()));

            if let Some(text) = text {
                if let Ok(customer) = state.customer_client.get_or_create_customer(sender_id, "facebook", None).await {
                    let payload = MessageServicePayload {
                        conversation_id: recipient_id.clone(),
                        customer_id: customer.id,
                        platform: "facebook".to_string(),
                        direction: direction.to_string(),
                        message_text: Some(text.clone()),
                        external_id: msg.mid.clone(),
                        created_at: chrono::Utc::now(),
                    };
                    let _ = state.message_client.store_message(payload).await;

                    if direction == "incoming" {
                        let _ = post_to_mattermost(&state, recipient_id, &text, msg.mid.as_deref()).await;
                    }
                }
            }
        }
    }

    Ok(StatusCode::OK)
}

async fn post_to_mattermost(
    state: &AppState,
    conversation_id: &str,
    text: &str,
    root_id: Option<&str>,
) -> Result<(), anyhow::Error> {
    let mm = &state.mattermost_client;
    if let Ok(team_id) = mm.get_team_id().await {
        if let Ok(channel_id) = mm.get_or_create_channel(&team_id, conversation_id, conversation_id).await {
            let root = if let Some(r) = root_id {
                Some(r.to_string())
            } else {
                mm.get_root_id(conversation_id).await.ok().flatten()
            };
            mm.post_message(&channel_id, text, root.as_deref(), None).await?;
        }
    }
    Ok(())
}

fn parse_webhook_entry(body: &str) -> Option<WebhookEntry> {
    serde_json::from_str(body).ok()
}

#[derive(Debug, Deserialize)]
pub struct WebhookVerificationParams {
    pub hub_mode: String,
    pub hub_verify_token: String,
    pub hub_challenge: String,
}

#[derive(Debug, Deserialize)]
pub struct WebhookEntry {
    pub messaging: Vec<WebhookMessaging>,
}

#[derive(Debug, Deserialize)]
pub struct WebhookMessaging {
    pub sender: WebhookSender,
    pub recipient: WebhookSender,
    pub message: Option<WebhookMessage>,
}

#[derive(Debug, Deserialize)]
pub struct WebhookSender {
    pub id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct WebhookMessage {
    pub mid: Option<String>,
    pub text: Option<String>,
    pub quick_reply: Option<WebhookQuickReply>,
}

#[derive(Debug, Deserialize)]
pub struct WebhookQuickReply {
    pub payload: String,
}

// Existing Handlers

/// Start import for all conversations
pub async fn import_all_conversations(
    State(state): State<AppState>,
) -> Result<Json<ImportResponse>, (StatusCode, String)> {
    info!("=== STARTING FACEBOOK CONVERSATIONS IMPORT ===");

    // Validate configuration
    if state.config.facebook_page_id.is_empty() {
        error!("FACEBOOK_PAGE_ID not configured");
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            "FACEBOOK_PAGE_ID environment variable must be set".to_string(),
        ));
    }

    if state.config.facebook_page_access_token.is_empty() {
        error!("FACEBOOK_PAGE_ACCESS_TOKEN not configured");
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            "FACEBOOK_PAGE_ACCESS_TOKEN environment variable must be set".to_string(),
        ));
    }

    let start_time = Instant::now();

    // Create import job
    let job_id = Uuid::new_v4();
    if let Err(e) = db::create_import_job(&state.pool, job_id, "running").await {
        error!("Failed to create import job: {}", e);
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Database error: {e}"),
        ));
    }

    // Update job as started
    if let Err(e) = db::update_import_job_started(&state.pool, job_id).await {
        error!("Failed to update import job: {}", e);
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Database error: {e}"),
        ));
    }

    // Fetch all conversations
    info!("Fetching conversations from Graph API...");
    let conversations = match graph_api::get_conversations(&state.pool, &state.config).await {
        Ok(convs) => convs,
        Err(e) => {
            error!("Failed to fetch conversations: {}", e);
            db::update_import_job_error(&state.pool, job_id, &e.to_string())
                .await
                .ok();
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to fetch conversations: {e}"),
            ));
        }
    };

    info!("Found {} total conversations", conversations.len());

    // Update job with total count
    db::update_import_job_totals(&state.pool, job_id, conversations.len() as i32)
        .await
        .ok();

    let mut processed = 0;
    let mut failed = 0;
    let mut total_messages = 0;
    let mut messages_stored = 0;
    let mut messages_skipped = 0;
    let mut errors: Vec<String> = Vec::new();

    // Process each conversation
    for (idx, conversation) in conversations.iter().enumerate() {
        info!(
            "Processing conversation {}/{}: {}",
            idx + 1,
            conversations.len(),
            conversation.id
        );

        // Create conversation import record
        let conv_import_id = Uuid::new_v4();
        db::create_conversation_import(&state.pool, conv_import_id, job_id, &conversation.id)
            .await
            .ok();

        db::update_conversation_import_started(&state.pool, conv_import_id)
            .await
            .ok();

        match process_conversation(
            &state,
            &conversation.id,
            &state.config.facebook_page_id,
            &state.config.facebook_page_access_token,
        )
        .await
        {
            Ok(result) => {
                processed += 1;
                total_messages += result.messages_fetched;
                messages_stored += result.messages_stored;
                messages_skipped += result.messages_skipped;

                db::update_conversation_import_completed(
                    &state.pool,
                    conv_import_id,
                    result.messages_fetched,
                    result.messages_stored,
                )
                .await
                .ok();

                info!(
                    "✅ Completed conversation {}: {} messages fetched, {} stored",
                    conversation.id, result.messages_fetched, result.messages_stored
                );
            }
            Err(e) => {
                failed += 1;
                let error_msg = format!("Conversation {}: {e}", conversation.id);
                errors.push(error_msg.clone());

                db::update_conversation_import_error(&state.pool, conv_import_id, &e.to_string())
                    .await
                    .ok();

                error!("❌ Failed conversation {}: {}", conversation.id, e);
            }
        }

        // Update job progress
        db::update_import_job_progress(
            &state.pool,
            job_id,
            processed,
            failed,
            total_messages,
            messages_stored,
            messages_skipped,
        )
        .await
        .ok();
    }

    // Mark job as completed
    let status = if errors.is_empty() {
        "completed"
    } else {
        "completed_with_errors"
    };

    db::update_import_job_completed(
        &state.pool,
        job_id,
        processed,
        failed,
        total_messages,
        messages_stored,
        messages_skipped,
        status,
    )
    .await
    .ok();

    let duration = start_time.elapsed().as_secs_f64();

    info!("=== IMPORT SUMMARY ===");
    info!("Status: {}", status);
    info!("Conversations processed: {}", processed);
    info!("Conversations failed: {}", failed);
    info!("Messages fetched: {}", total_messages);
    info!("Messages stored: {}", messages_stored);
    info!("Messages skipped: {}", messages_skipped);
    info!("Duration: {:.2}s", duration);

    Ok(Json(ImportResponse {
        status: status.to_string(),
        job_id,
        message: format!(
            "Import completed: {processed} processed, {failed} failed, {messages_stored} messages stored in {duration:.2}s"
        ),
    }))
}

/// Import a single conversation by ID
pub async fn import_single_conversation(
    State(state): State<AppState>,
    axum::extract::Path(conversation_id): axum::extract::Path<String>,
) -> Result<Json<ConversationImportResult>, (StatusCode, String)> {
    info!("Importing single conversation: {}", conversation_id);

    let result = process_conversation(
        &state,
        &conversation_id,
        &state.config.facebook_page_id,
        &state.config.facebook_page_access_token,
    )
    .await
    .map_err(|e| {
        error!("Failed to import conversation: {}", e);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Import failed: {e}"),
        )
    })?;

    Ok(Json(result))
}

/// Get import status
pub async fn get_import_status(
    State(state): State<AppState>,
) -> Result<Json<ImportStatusResponse>, (StatusCode, String)> {
    let status = db::get_latest_import_status(&state.pool)
        .await
        .map_err(|e| {
            error!("Failed to get import status: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Database error: {e}"),
            )
        })?;

    Ok(Json(status))
}

/// Exchange short-lived token for long-lived token
/// POST /api/token/exchange
pub async fn exchange_token(
    State(state): State<AppState>,
    Json(request): Json<TokenExchangeRequest>,
) -> Result<Json<TokenExchangeResponse>, (StatusCode, String)> {
    info!("Received token exchange request");

    // Validate that app credentials are configured
    if state.config.facebook_app_id.is_empty() {
        error!("FACEBOOK_APP_ID not configured");
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            "FACEBOOK_APP_ID environment variable must be set".to_string(),
        ));
    }

    if state.config.facebook_app_secret.is_empty() {
        error!("FACEBOOK_APP_SECRET not configured");
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            "FACEBOOK_APP_SECRET environment variable must be set".to_string(),
        ));
    }

    // Perform token exchange
    match graph_api::exchange_token_for_long_lived(
        &request.short_lived_token,
        &state.config.facebook_app_id,
        &state.config.facebook_app_secret,
    )
    .await
    {
        Ok(response) => {
            info!(
                "Successfully exchanged token, expires in {} seconds",
                response.expires_in
            );
            Ok(Json(TokenExchangeResponse {
                access_token: response.access_token,
                token_type: response.token_type,
                expires_in: response.expires_in,
            }))
        }
        Err(e) => {
            error!("Token exchange failed: {}", e);
            Err((
                StatusCode::BAD_REQUEST,
                format!("Token exchange failed: {e}"),
            ))
        }
    }
}

/// Process a single conversation - fetch messages and store via services
async fn process_conversation(
    state: &AppState,
    conversation_id: &str,
    page_id: &str,
    access_token: &str,
) -> anyhow::Result<ConversationImportResult> {
    info!("Processing conversation ID: {}", conversation_id);

    // Fetch all messages for this conversation
    let messages = graph_api::get_conversation_messages(&state.pool, conversation_id, access_token)
        .await
        .context("Failed to fetch messages from Graph API")?;

    let total_messages = messages.len();
    info!(
        "Fetched {} messages for conversation {}",
        total_messages, conversation_id
    );

    let mut messages_stored = 0;
    let mut messages_skipped = 0;

    // Process each message
    for msg in &messages {
        // Determine direction: if message is from the page, it's outgoing; otherwise incoming
        let is_from_page = msg.from.id == page_id;
        let direction = if is_from_page { "outgoing" } else { "incoming" };

        // Get customer info
        let (customer_platform_id, customer_name): (Option<String>, Option<String>) =
            if is_from_page {
                // For outgoing messages, customer is the recipient
                (
                    msg.to.data.first().map(|u| u.id.clone()),
                    msg.to.data.first().map(|u| u.name.clone()),
                )
            } else {
                // For incoming messages, customer is the sender
                (Some(msg.from.id.clone()), Some(msg.from.name.clone()))
            };

        let cust_id = match customer_platform_id {
            Some(id) => id,
            None => {
                warn!("Skipping message {}: no customer ID found", msg.id);
                continue;
            }
        };

        // Get or create customer via Customer Service
        let customer = match state
            .customer_client
            .get_or_create_customer(&cust_id, "facebook", customer_name.as_deref())
            .await
        {
            Ok(c) => {
                debug!("Customer resolved: {} (ID: {})", c.platform_user_id, c.id);
                c
            }
            Err(e) => {
                error!("Failed to get/create customer {}: {}", cust_id, e);
                continue;
            }
        };

        // Store message via Message Service
        let message_payload = MessageServicePayload {
            conversation_id: conversation_id.to_string(),
            customer_id: customer.id,
            platform: "facebook".to_string(),
            direction: direction.to_string(),
            message_text: msg.message.clone(),
            external_id: Some(msg.id.clone()),
            created_at: msg.created_time,
        };

        match state.message_client.store_message(message_payload).await {
            Ok(_) => {
                messages_stored += 1;
                // Attempt to post to Mattermost as a normal conversation (threaded)
                // Best-effort: log and continue on failure
                let mm = &state.mattermost_client;
                // Ensure token and fetch team/channel, post message
                if let Ok(team_id) = mm.get_team_id().await {
                    if let Ok(channel_id) = mm
                        .get_or_create_channel(&team_id, conversation_id, conversation_id)
                        .await
                    {
                        let root_id_opt = mm.get_root_id(conversation_id).await?;
                        let root_id_slice = root_id_opt.as_deref();
                        match mm
                            .post_message(
                                &channel_id,
                                msg.message.as_deref().unwrap_or(""),
                                root_id_slice,
                                Some(msg.created_time.timestamp_millis()),
                            )
                            .await
                        {
                            Ok(post_id) => {
                                if root_id_opt.is_none() {
                                    // store root_id for threading
                                    mm.set_root_id(conversation_id, &post_id);
                                }
                            }
                            Err(e) => {
                                warn!(
                                    "Mattermost post failed for conversation {}: {}",
                                    conversation_id, e
                                );
                            }
                        }
                    }
                } else {
                    warn!(
                        "Could not determine Mattermost team_id for conversation {}",
                        conversation_id
                    );
                }
            }
            Err(e) if e.to_string().contains("already exists") => {
                // Duplicate - expected on re-runs
                messages_skipped += 1;
            }
            Err(e) => {
                error!("Failed to store message {}: {}", msg.id, e);
                // Continue processing other messages
            }
        }
    }

    Ok(ConversationImportResult {
        conversation_id: conversation_id.to_string(),
        status: "completed".to_string(),
        messages_fetched: total_messages as i32,
        messages_stored,
        messages_skipped,
        error: None,
    })
}
