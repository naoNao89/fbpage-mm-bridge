//! HTTP handlers for the Facebook Graph Service

use anyhow::Context;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
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
    info!("Received webhook event: {}", &body);

    let payload = match parse_webhook_entry(&body) {
        Some(p) => {
            debug!("Parsed payload successfully");
            p
        }
        None => {
            error!("Failed to parse webhook payload. Body was: {}", &body);
            return Ok(StatusCode::OK);
        }
    };

    for entry in payload.entry {
        debug!("Processing entry: {}", entry.id);
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

            let is_echo = msg.is_echo.unwrap_or(false);

            // For echo messages (page's own replies), the customer is the recipient.
            // For incoming messages (from user), the customer is the sender.
            let (customer_id, conversation_id) = if is_echo {
                (recipient_id, recipient_id)
            } else {
                (sender_id, recipient_id)
            };

            let direction = if is_echo { "outgoing" } else { "incoming" };

            let text = msg
                .text
                .clone()
                .or_else(|| msg.quick_reply.as_ref().map(|q| q.payload.clone()));

            let has_attachments = msg
                .attachments
                .as_ref()
                .map(|a| !a.is_empty())
                .unwrap_or(false);

            let final_text = if let Some(text) = text {
                if has_attachments {
                    let att_md = format_webhook_attachments(msg.attachments.as_deref());
                    Some(if text.trim().is_empty() { att_md } else { format!("{text}\n{att_md}") })
                } else {
                    Some(text)
                }
            } else if has_attachments {
                Some(format_webhook_attachments(msg.attachments.as_deref()))
            } else {
                None
            };

            if let Some(text) = final_text {
                if let Ok(customer) = state
                    .customer_client
                    .get_or_create_customer(customer_id, "facebook", None)
                    .await
                {
                    let payload = MessageServicePayload {
                        conversation_id: conversation_id.to_string(),
                        customer_id: customer.id,
                        platform: "facebook".to_string(),
                        direction: direction.to_string(),
                        message_text: Some(text.clone()),
                        external_id: msg.mid.clone(),
                        created_at: chrono::Utc::now(),
                    };
                    let _ = state.message_client.store_message(payload).await;

                    if let Some(ref mid) = msg.mid {
                        if !state.mattermost_client.mark_posted(mid) {
                            continue;
                        }
                    }

                    let display_name = customer.name.as_deref().unwrap_or(conversation_id);
                    let _ = post_to_mattermost(
                        &state,
                        conversation_id,
                        &text,
                        msg.mid.as_deref(),
                        display_name,
                        direction,
                        Some(customer_id),
                    )
                    .await;
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
    display_name: &str,
    direction: &str,
    customer_platform_id: Option<&str>,
) -> Result<(), anyhow::Error> {
    let mm = &state.mattermost_client;
    let team_id = match mm.get_team_id().await {
        Ok(id) => id,
        Err(e) => {
            warn!(
                "Could not determine Mattermost team_id for conversation {}: {e}",
                conversation_id
            );
            return Ok(());
        }
    };
    if let Ok(channel_id) = mm
        .get_or_create_channel(&team_id, conversation_id, display_name)
        .await
    {
        let _ = mm
            .maybe_update_display_name(&channel_id, conversation_id, display_name)
            .await;

        if direction == "incoming" {
            if let Some(psid) = customer_platform_id {
                match mm
                    .get_or_create_customer_bot(psid, display_name, &channel_id)
                    .await
                {
                    Ok((bot_user_id, bot_token)) => {
                        let root = if let Some(r) = root_id {
                            Some(r.to_string())
                        } else {
                            mm.get_root_id(conversation_id).await.ok().flatten()
                        };
                        match mm
                            .post_message_as_bot(
                                &channel_id,
                                text,
                                root.as_deref(),
                                None,
                                &bot_user_id,
                                &bot_token,
                            )
                            .await
                        {
                            Ok(post_id) => {
                                if root.is_none() {
                                    mm.set_root_id(conversation_id, &post_id);
                                }
                            }
                            Err(e) => {
                                warn!(
                                    "Bot post failed for conversation {}, falling back to admin: {e}",
                                    conversation_id
                                );
                                let root = if let Some(r) = root_id {
                                    Some(r.to_string())
                                } else {
                                    mm.get_root_id(conversation_id).await.ok().flatten()
                                };
                                mm.post_message(&channel_id, text, root.as_deref(), None)
                                    .await?;
                            }
                        }
                    }
                    Err(e) => {
                        warn!(
                            "Failed to create customer bot for {}, falling back to admin: {e}",
                            psid
                        );
                        let root = if let Some(r) = root_id {
                            Some(r.to_string())
                        } else {
                            mm.get_root_id(conversation_id).await.ok().flatten()
                        };
                        mm.post_message(&channel_id, text, root.as_deref(), None)
                            .await?;
                    }
                }
            } else {
                let root = if let Some(r) = root_id {
                    Some(r.to_string())
                } else {
                    mm.get_root_id(conversation_id).await.ok().flatten()
                };
                mm.post_message(&channel_id, text, root.as_deref(), None)
                    .await?;
            }
        } else {
            let root = if let Some(r) = root_id {
                Some(r.to_string())
            } else {
                mm.get_root_id(conversation_id).await.ok().flatten()
            };
            mm.post_message(&channel_id, text, root.as_deref(), None)
                .await?;
        }
    }
    Ok(())
}

fn parse_webhook_entry(body: &str) -> Option<WebhookPayload> {
    serde_json::from_str(body).ok()
}

fn format_webhook_attachments(attachments: Option<&[WebhookAttachment]>) -> String {
    let Some(attachments) = attachments else {
        return String::new();
    };
    let mut parts = Vec::new();
    for att in attachments {
        let att_type = att.attachment_type.as_deref().unwrap_or("file");
        let url = att
            .payload
            .as_ref()
            .and_then(|p| p.url.as_deref());
        match (att_type, url) {
            ("image", Some(url)) => parts.push(format!("![image]({url})")),
            ("image", None) => parts.push("📷 [image]".to_string()),
            ("video", Some(url)) => parts.push(format!("[▶ video]({url})")),
            ("video", None) => parts.push("📹 [video]".to_string()),
            ("audio", Some(url)) => parts.push(format!("[🎵 audio]({url})")),
            ("audio", None) => parts.push("🎵 [audio]".to_string()),
            (_, Some(url)) => parts.push(format!("[📎 file]({url})")),
            _ => parts.push("📎 [file]".to_string()),
        }
    }
    parts.join("\n")
}

#[derive(Debug, Deserialize)]
pub struct WebhookVerificationParams {
    #[serde(rename = "hub.mode")]
    pub hub_mode: String,
    #[serde(rename = "hub.verify_token")]
    pub hub_verify_token: String,
    #[serde(rename = "hub.challenge")]
    pub hub_challenge: String,
}

#[derive(Debug, Deserialize)]
pub struct WebhookPayload {
    pub object: String,
    pub entry: Vec<WebhookEntry>,
}

#[derive(Debug, Deserialize)]
pub struct WebhookEntry {
    pub id: String,
    pub time: i64,
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
    pub is_echo: Option<bool>,
    pub quick_reply: Option<WebhookQuickReply>,
    #[serde(default)]
    pub attachments: Option<Vec<WebhookAttachment>>,
}

#[derive(Debug, Deserialize)]
pub struct WebhookAttachment {
    #[serde(rename = "type")]
    pub attachment_type: Option<String>,
    pub payload: Option<WebhookAttachmentPayload>,
}

#[derive(Debug, Deserialize)]
pub struct WebhookAttachmentPayload {
    pub url: Option<String>,
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
pub async fn process_conversation(
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
            Ok(msg_resp) => {
                if !state.mattermost_client.mark_posted(&msg.id) {
                    continue;
                }
                messages_stored += 1;
                // Attempt to post to Mattermost as a normal conversation (threaded)
                // Best-effort: log and continue on failure
                let mm = &state.mattermost_client;
                // Ensure token and fetch team/channel, post message
                if let Ok(team_id) = mm.get_team_id().await {
                    let display_name = customer.name.as_deref().unwrap_or(conversation_id);
                    if let Ok(channel_id) = mm
                        .get_or_create_channel(&team_id, conversation_id, display_name)
                        .await
                    {
                        let _ = mm
                            .maybe_update_display_name(&channel_id, conversation_id, display_name)
                            .await;
                        let root_id_opt = mm.get_root_id(conversation_id).await?;
                        let root_id_slice = root_id_opt.as_deref();
                        let msg_text = msg.message.as_deref().unwrap_or("");
                        let attachments = crate::media::extract_attachments_from_graph(msg);
                        let ts = Some(msg.created_time.timestamp_millis());

                        if direction == "incoming" {
                            match mm
                                .get_or_create_customer_bot(&cust_id, display_name, &channel_id)
                                .await
                            {
                                Ok((bot_uid, bot_token)) => {
                                    let (final_text, file_ids) = if !attachments.is_empty() {
                                        crate::media::process_attachments_for_post(
                                            state, mm, &channel_id, msg_text, &attachments,
                                            &msg.id, Some(msg_resp.id), Some(&bot_token),
                                        ).await
                                    } else {
                                        (msg_text.to_string(), Vec::new())
                                    };

                                    let result = if file_ids.is_empty() {
                                        mm.post_message_as_bot(
                                            &channel_id,
                                            &final_text,
                                            root_id_slice,
                                            ts,
                                            &bot_uid,
                                            &bot_token,
                                        )
                                        .await
                                    } else {
                                        mm.post_message_as_bot_with_files(
                                            &channel_id,
                                            &final_text,
                                            root_id_slice,
                                            ts,
                                            &bot_uid,
                                            &bot_token,
                                            &file_ids,
                                        )
                                        .await
                                    };
                                    match result {
                                        Ok(post_id) => {
                                            if root_id_opt.is_none() {
                                                mm.set_root_id(conversation_id, &post_id);
                                            }
                                        }
                                        Err(e) => {
                                            warn!("Bot post failed, falling back: {e}");
                                            if let Ok(post_id) = mm
                                                .post_message(
                                                    &channel_id,
                                                    &final_text,
                                                    root_id_slice,
                                                    ts,
                                                )
                                                .await
                                            {
                                                if root_id_opt.is_none() {
                                                    mm.set_root_id(conversation_id, &post_id);
                                                }
                                            }
                                        }
                                    }
                                }
                                Err(e) => {
                                    warn!("Bot creation failed, falling back: {e}");
                                    if let Ok(post_id) = mm
                                        .post_message(&channel_id, msg_text, root_id_slice, ts)
                                        .await
                                    {
                                        if root_id_opt.is_none() {
                                            mm.set_root_id(conversation_id, &post_id);
                                        }
                                    }
                                }
                            }
                        } else {
                            let (final_text, file_ids) = if !attachments.is_empty() {
                                crate::media::process_attachments_for_post(
                                    state, mm, &channel_id, msg_text, &attachments,
                                    &msg.id, Some(msg_resp.id), None,
                                ).await
                            } else {
                                (msg_text.to_string(), Vec::new())
                            };

                            match mm
                                .post_message_with_files(&channel_id, &final_text, root_id_slice, ts, &file_ids)
                                .await
                            {
                                Ok(post_id) => {
                                    if root_id_opt.is_none() {
                                        mm.set_root_id(conversation_id, &post_id);
                                    }
                                }
                                Err(e) => {
                                    warn!("Mattermost post failed for {}: {}", conversation_id, e);
                                }
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

/// Re-import a single conversation: delete all existing posts, then re-post
/// with direction-aware bot names (incoming → customer bot, outgoing → admin).
pub async fn reimport_conversation(
    State(state): State<AppState>,
    Path(conversation_id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let mm = &state.mattermost_client;
    let team_id = mm
        .get_team_id()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // Get or create channel
    let channel_id = mm
        .get_or_create_channel(&team_id, &conversation_id, &conversation_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // Delete all existing posts in the channel
    let auth = mm
        .get_auth_header()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let client = reqwest::Client::new();
    let mut deleted = 0u32;
    let mut page = 0u32;
    let base = state.config.mattermost_url.trim_end_matches('/');
    loop {
        let url = format!("{base}/api/v4/channels/{channel_id}/posts?per_page=200&page={page}",);
        let resp = client
            .get(&url)
            .header("Authorization", format!("Bearer {auth}"))
            .send()
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

        if !resp.status().is_success() {
            break;
        }

        let data: serde_json::Value = resp.json().await.unwrap_or_default();
        let posts = match data.get("posts").and_then(|p| p.as_object()) {
            Some(p) => p,
            None => break,
        };

        if posts.is_empty() {
            break;
        }

        for (_, post) in posts {
            if let Some(pid) = post.get("id").and_then(|i| i.as_str()) {
                let del_url = format!("{base}/api/v4/posts/{pid}");
                let _ = client
                    .delete(&del_url)
                    .header("Authorization", format!("Bearer {auth}"))
                    .send()
                    .await;
                deleted += 1;
            }
        }

        page += 1;
    }

    tracing::info!(
        "Reimport: deleted {} posts from channel {}",
        deleted,
        conversation_id
    );

    // Fetch messages from Facebook Graph API
    let messages = crate::graph_api::get_conversation_messages(
        &state.pool,
        &conversation_id,
        &state.config.facebook_page_access_token,
    )
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let total = messages.len();
    tracing::info!(
        "Reimport: fetched {} messages for conversation {}",
        total,
        conversation_id
    );

    // Sort: incoming first, then outgoing
    let mut messages = messages;
    messages.sort_by_key(|m| m.created_time);

    let display_name = messages
        .iter()
        .find_map(|msg| {
            if msg.from.id != state.config.facebook_page_id {
                Some(msg.from.name.clone())
            } else {
                msg.to.data.first().map(|u| u.name.clone())
            }
        })
        .unwrap_or_else(|| conversation_id.to_string());

    let _ = mm
        .maybe_update_display_name(&channel_id, &conversation_id, &display_name)
        .await;

    let mut posted = 0u32;
    let mut root_id: Option<String> = None;

    // Post all incoming (customer) messages first to ensure the thread root is from the customer
    for msg in &messages {
        if msg.from.id == state.config.facebook_page_id {
            continue;
        }
        let text = match &msg.message {
            Some(t) if !t.trim().is_empty() => t.as_str(),
            _ => continue,
        };
        let customer_name = &msg.from.name;
        let cust_id = &msg.from.id;

        match mm
            .get_or_create_customer_bot(cust_id, customer_name, &channel_id)
            .await
        {
            Ok((bot_uid, bot_token)) => {
                let root = root_id.as_deref();
                match mm
                    .post_message_as_bot(&channel_id, text, root, None, &bot_uid, &bot_token)
                    .await
                {
                    Ok(post_id) => {
                        if root_id.is_none() {
                            mm.set_root_id(&conversation_id, &post_id);
                            root_id = Some(post_id);
                        }
                        posted += 1;
                    }
                    Err(e) => {
                        tracing::warn!("Reimport: bot post failed for {}: {}", conversation_id, e);
                        let root = root_id.as_deref();
                        if let Ok(post_id) = mm.post_message(&channel_id, text, root, None).await {
                            if root_id.is_none() {
                                mm.set_root_id(&conversation_id, &post_id);
                                root_id = Some(post_id);
                            }
                            posted += 1;
                        }
                    }
                }
            }
            Err(e) => {
                tracing::warn!("Reimport: bot creation failed for {}: {}", cust_id, e);
                let root = root_id.as_deref();
                if let Ok(post_id) = mm.post_message(&channel_id, text, root, None).await {
                    if root_id.is_none() {
                        mm.set_root_id(&conversation_id, &post_id);
                        root_id = Some(post_id);
                    }
                    posted += 1;
                }
            }
        }
    }

    // Now post outgoing (page) messages
    for msg in &messages {
        if msg.from.id != state.config.facebook_page_id {
            continue;
        }
        let text = match &msg.message {
            Some(t) if !t.trim().is_empty() => t.as_str(),
            _ => continue,
        };
        let root = root_id.as_deref();
        if let Ok(post_id) = mm.post_message(&channel_id, text, root, None).await {
            if root_id.is_none() {
                mm.set_root_id(&conversation_id, &post_id);
                root_id = Some(post_id);
            }
            posted += 1;
        }
    }

    Ok(Json(serde_json::json!({
        "conversation_id": conversation_id,
        "deleted_posts": deleted,
        "messages_fetched": total,
        "messages_posted": posted,
        "status": "completed"
    })))
}

pub async fn reimport_all_conversations(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let channels = state
        .mattermost_client
        .get_all_t_channels()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let total = channels.len();
    info!(
        "Reimport-all: starting background reimport of {} channels",
        total
    );

    tokio::spawn(async move {
        let mut total_deleted = 0u32;
        let mut total_fetched = 0usize;
        let mut total_posted = 0u32;
        let mut errors = 0u32;

        for (idx, channel) in channels.iter().enumerate() {
            let conv_id = &channel.name;
            info!(
                "Reimport-all [{}/{}]: processing {}",
                idx + 1,
                total,
                conv_id
            );

            match reimport_single_conversation(&state, conv_id, &channel.id).await {
                Ok(result) => {
                    total_deleted += result.deleted_posts;
                    total_fetched += result.messages_fetched;
                    total_posted += result.messages_posted;
                }
                Err(e) => {
                    tracing::error!("Reimport-all: failed for {}: {}", conv_id, e);
                    errors += 1;
                }
            }
        }

        info!(
            "Reimport-all complete: {} channels, {} deleted, {} fetched, {} posted, {} errors",
            total, total_deleted, total_fetched, total_posted, errors
        );
    });

    Ok(Json(serde_json::json!({
        "status": "started",
        "total_channels": total,
        "message": "Reimport running in background. Check logs for progress."
    })))
}

struct ReimportResult {
    deleted_posts: u32,
    messages_fetched: usize,
    messages_posted: u32,
}

async fn reimport_single_conversation(
    state: &AppState,
    conversation_id: &str,
    channel_id: &str,
) -> Result<ReimportResult, anyhow::Error> {
    let mm = &state.mattermost_client;

    let auth = mm.get_auth_header().await?;
    let client = reqwest::Client::new();
    let mut deleted = 0u32;
    let mut page = 0u32;
    let base = state.config.mattermost_url.trim_end_matches('/');
    loop {
        let url = format!("{base}/api/v4/channels/{channel_id}/posts?per_page=200&page={page}",);
        let resp = client
            .get(&url)
            .header("Authorization", format!("Bearer {auth}"))
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to fetch posts: {e}"))?;

        if !resp.status().is_success() {
            break;
        }

        let data: serde_json::Value = resp.json().await.unwrap_or_default();
        let posts = match data.get("posts").and_then(|p| p.as_object()) {
            Some(p) => p,
            None => break,
        };

        if posts.is_empty() {
            break;
        }

        for (_, post) in posts {
            if let Some(pid) = post.get("id").and_then(|i| i.as_str()) {
                let del_url = format!("{base}/api/v4/posts/{pid}");
                let _ = client
                    .delete(&del_url)
                    .header("Authorization", format!("Bearer {auth}"))
                    .send()
                    .await;
                deleted += 1;
            }
        }

        page += 1;
    }

    tracing::info!(
        "Reimport: deleted {} posts from channel {}",
        deleted,
        conversation_id
    );

    let messages = crate::graph_api::get_conversation_messages(
        &state.pool,
        conversation_id,
        &state.config.facebook_page_access_token,
    )
    .await
    .map_err(|e| anyhow::anyhow!("Failed to fetch messages: {e}"))?;

    let total = messages.len();
    tracing::info!(
        "Reimport: fetched {} messages for conversation {}",
        total,
        conversation_id
    );

    let mut messages = messages;
    messages.sort_by_key(|m| m.created_time);
    let display_name = messages
        .iter()
        .find_map(|msg| {
            if msg.from.id != state.config.facebook_page_id {
                Some(msg.from.name.clone())
            } else {
                msg.to.data.first().map(|u| u.name.clone())
            }
        })
        .unwrap_or_else(|| conversation_id.to_string());

    let _ = mm
        .maybe_update_display_name(channel_id, conversation_id, &display_name)
        .await;

    let mut posted = 0u32;
    let mut root_id: Option<String> = None;

    // Post all incoming (customer) messages first to ensure the thread root is from the customer
    for msg in &messages {
        if msg.from.id == state.config.facebook_page_id {
            continue;
        }
        let text = msg.message.as_deref().unwrap_or("");
        let attachments = crate::media::extract_attachments_from_graph(msg);
        if text.trim().is_empty() && attachments.is_empty() {
            continue;
        }
        let customer_name = &msg.from.name;
        let cust_id = &msg.from.id;

        match mm
            .get_or_create_customer_bot(cust_id, customer_name, channel_id)
            .await
        {
            Ok((bot_uid, bot_token)) => {
                let (final_text, file_ids) = if !attachments.is_empty() {
                    crate::media::process_attachments_for_post(
                        state, mm, channel_id, text, &attachments,
                        &msg.id, None, Some(&bot_token),
                    ).await
                } else {
                    (text.to_string(), Vec::new())
                };

                let root = root_id.as_deref();
                let result = if file_ids.is_empty() {
                    mm.post_message_as_bot(channel_id, &final_text, root, None, &bot_uid, &bot_token).await
                } else {
                    mm.post_message_as_bot_with_files(channel_id, &final_text, root, None, &bot_uid, &bot_token, &file_ids).await
                };
                match result {
                    Ok(post_id) => {
                        if root_id.is_none() {
                            mm.set_root_id(conversation_id, &post_id);
                            root_id = Some(post_id);
                        }
                        posted += 1;
                    }
                    Err(e) => {
                        tracing::warn!("Reimport: bot post failed for {}: {}", conversation_id, e);
                        let root = root_id.as_deref();
                        if let Ok(post_id) = mm.post_message(channel_id, &final_text, root, None).await {
                            if root_id.is_none() {
                                mm.set_root_id(conversation_id, &post_id);
                                root_id = Some(post_id);
                            }
                            posted += 1;
                        }
                    }
                }
            }
            Err(e) => {
                tracing::warn!("Reimport: bot creation failed for {}: {}", cust_id, e);
                let root = root_id.as_deref();
                if let Ok(post_id) = mm.post_message(channel_id, text, root, None).await {
                    if root_id.is_none() {
                        mm.set_root_id(conversation_id, &post_id);
                        root_id = Some(post_id);
                    }
                    posted += 1;
                }
            }
        }
    }

    // Now post outgoing (page) messages
    for msg in &messages {
        if msg.from.id != state.config.facebook_page_id {
            continue;
        }
        let text = msg.message.as_deref().unwrap_or("");
        let attachments = crate::media::extract_attachments_from_graph(msg);
        if text.trim().is_empty() && attachments.is_empty() {
            continue;
        }
        let (final_text, file_ids) = if !attachments.is_empty() {
            crate::media::process_attachments_for_post(
                state, mm, channel_id, text, &attachments,
                &msg.id, None, None,
            ).await
        } else {
            (text.to_string(), Vec::new())
        };

        let root = root_id.as_deref();
        let result = if file_ids.is_empty() {
            mm.post_message(channel_id, &final_text, root, None).await
        } else {
            mm.post_message_with_files(channel_id, &final_text, root, None, &file_ids).await
        };
        if let Ok(post_id) = result {
            if root_id.is_none() {
                mm.set_root_id(conversation_id, &post_id);
                root_id = Some(post_id);
            }
            posted += 1;
        }
    }

    Ok(ReimportResult {
        deleted_posts: deleted,
        messages_fetched: total,
        messages_posted: posted,
    })
}
