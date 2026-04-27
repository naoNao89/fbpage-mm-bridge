//! HTTP handlers for the Facebook Graph Service

use anyhow::Context;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};
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

pub async fn instagram_webhook_verification(
    State(state): State<AppState>,
    axum::extract::Query(params): axum::extract::Query<WebhookVerificationParams>,
) -> Result<String, (StatusCode, String)> {
    if params.hub_mode != "subscribe" {
        return Err((StatusCode::BAD_REQUEST, "Invalid hub.mode".to_string()));
    }
    if params.hub_verify_token != state.config.instagram_webhook_verify_token {
        return Err((StatusCode::FORBIDDEN, "Invalid verify token".to_string()));
    }
    Ok(params.hub_challenge)
}

pub async fn instagram_webhook_handler(
    State(state): State<AppState>,
    body: String,
) -> Result<StatusCode, (StatusCode, String)> {
    info!("Received Instagram webhook event: {}", &body);

    let ig_user_id = state.config.instagram_ig_user_id.clone();
    let access_token = state.config.facebook_page_access_token.clone();
    tokio::spawn(async move {
        let client = reqwest::Client::new();
        let url = format!(
            "https://graph.facebook.com/v24.0/{ig_user_id}/subscribed_apps?access_token={access_token}"
        );
        let payload = serde_json::json!({
            "subscribed_fields": ["messages", "messaging_postbacks", "messaging_referrals"]
        });
        if let Ok(resp) = client.post(&url).json(&payload).send().await {
            if resp.status().is_success() {
                debug!("Resubscribed Instagram webhook events");
            }
        }
    });

    let payload = match serde_json::from_str::<InstagramWebhookPayload>(&body) {
        Ok(p) => p,
        Err(e) => {
            error!("Failed to parse Instagram webhook payload: {e}");
            return Ok(StatusCode::OK);
        }
    };

    for entry in payload.entry {
        info!("Processing Instagram entry: {}", entry.id);
        for msg_event in entry.messaging {
            info!("Instagram messaging event: sender={:?}, recipient={:?}, message={:?}, postback={:?}", 
                  msg_event.sender, msg_event.recipient, msg_event.message, msg_event.postback);
            let sender_id = match &msg_event.sender.id {
                Some(id) => id,
                None => continue,
            };
            let recipient_id = match &msg_event.recipient.id {
                Some(id) => id,
                None => continue,
            };

            let is_echo = msg_event
                .message
                .as_ref()
                .and_then(|m| m.is_echo)
                .unwrap_or(false);

            let (customer_id, direction) = if is_echo {
                (recipient_id.clone(), "outgoing")
            } else {
                (sender_id.clone(), "incoming")
            };

            let text: Option<String>;
            let external_id: Option<String>;

            if let Some(ref message) = msg_event.message {
                text = message.text.clone();
                external_id = message.mid.clone();
            } else if let Some(ref postback) = msg_event.postback {
                text = Some(
                    postback
                        .title
                        .clone()
                        .unwrap_or_else(|| postback.payload.clone()),
                );
                external_id = None;
            } else {
                info!("Instagram: No message or postback found, skipping");
                continue;
            }

            info!(
                "Instagram: customer_id={}, text={:?}, external_id={:?}",
                customer_id, text, external_id
            );

            if let Some(msg_text) = text {
                info!("Instagram: Calling customer_client.get_or_create_customer");
                match state
                    .customer_client
                    .get_or_create_customer(&customer_id, "instagram", None)
                    .await
                {
                    Ok(customer) => {
                        info!("Instagram: Got customer: {:?}", customer);
                        let conv_id = format!("ig_{customer_id}");
                        let payload = MessageServicePayload {
                            conversation_id: conv_id.clone(),
                            customer_id: customer.id,
                            platform: "instagram".to_string(),
                            direction: direction.to_string(),
                            message_text: Some(msg_text.clone()),
                            external_id: external_id.clone(),
                            created_at: chrono::Utc::now(),
                        };
                        info!("Instagram: Storing message to message_service");
                        let store_result = state.message_client.store_message(payload).await;
                        let msg_id = match store_result {
                            Ok(msg) => msg.id,
                            Err(e) => {
                                warn!("Instagram: Failed to store message: {}", e);
                                continue;
                            }
                        };

                        let display_name = customer.name.as_deref().unwrap_or(&customer_id);
                        info!(
                            "Instagram: Posting to Mattermost, conv_id={}, display_name={}",
                            conv_id, display_name
                        );

                        let team_id = match state.mattermost_client.get_team_id().await {
                            Ok(id) => id,
                            Err(e) => {
                                warn!("Instagram: Could not determine Mattermost team_id: {}", e);
                                continue;
                            }
                        };

                        let channel_id = match state
                            .mattermost_client
                            .get_or_create_channel(&team_id, &conv_id, display_name)
                            .await
                        {
                            Ok(cid) => cid,
                            Err(e) => {
                                warn!("Instagram: Failed to get/create channel: {}", e);
                                continue;
                            }
                        };

                        let post_result = state
                            .mattermost_client
                            .post_message_with_override(
                                &channel_id,
                                &msg_text,
                                None,
                                None,
                                Some(display_name),
                                None,
                            )
                            .await;

                        if post_result.is_ok() {
                            if let Err(e) =
                                state.message_client.mark_synced(msg_id, &channel_id).await
                            {
                                warn!("Instagram: Failed to mark message as synced: {}", e);
                            }
                        }
                    }
                    Err(e) => {
                        error!("Instagram: Failed to get/create customer: {:?}", e);
                    }
                }
            }
        }
    }

    Ok(StatusCode::OK)
}

#[derive(Debug, Deserialize)]
pub struct InstagramWebhookPayload {
    pub object: String,
    pub entry: Vec<InstagramWebhookEntry>,
}

#[derive(Debug, Deserialize)]
pub struct InstagramWebhookEntry {
    pub id: String,
    pub time: i64,
    pub messaging: Vec<InstagramWebhookMessaging>,
}

#[derive(Debug, Deserialize)]
pub struct InstagramWebhookMessaging {
    pub sender: WebhookSender,
    pub recipient: WebhookSender,
    pub message: Option<InstagramWebhookMessage>,
    pub postback: Option<WebhookPostback>,
}

#[derive(Debug, Deserialize)]
pub struct InstagramWebhookMessage {
    pub mid: Option<String>,
    pub text: Option<String>,
    pub is_echo: Option<bool>,
}

pub async fn webhook_handler(
    State(state): State<AppState>,
    body: String,
) -> Result<StatusCode, (StatusCode, String)> {
    info!("Received webhook event: {}", &body);

    let access_token = state.config.facebook_page_access_token.clone();
    tokio::spawn(async move {
        let client = reqwest::Client::new();
        let url = format!(
            "https://graph.facebook.com/v24.0/me/subscribed_apps?access_token={access_token}"
        );
        let payload = serde_json::json!({
            "subscribed_fields": ["messages", "messaging_postbacks", "messaging_referrals"]
        });
        if let Ok(resp) = client.post(&url).json(&payload).send().await {
            if resp.status().is_success() {
                debug!("Resubscribed to webhook events");
            }
        }
    });

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
        for messaging in entry.messaging {
            let sender_id = match &messaging.sender.id {
                Some(id) => id,
                None => continue,
            };
            let recipient_id = match &messaging.recipient.id {
                Some(id) => id,
                None => continue,
            };

            let is_echo = messaging
                .message
                .as_ref()
                .and_then(|m| m.is_echo)
                .unwrap_or(false);

            // Customer PSID and conversation_id:
            // - For incoming (customer→page): customer_id = sender (PSID), customer is the conversation partner
            // - For echo (page→customer): customer_id = recipient (PSID), customer is still the conversation partner
            // - For postback (customer clicked button): customer_id = sender (PSID)
            // conversation_id should use the customer PSID consistently so that
            // webhook-driven and poller/import paths land in the same Mattermost channel.
            let (customer_id, direction) = if is_echo {
                (recipient_id, "outgoing")
            } else {
                (sender_id, "incoming")
            };

            // Resolve conversation_id from customer PSID via Graph API with cache
            let conversation_id = match resolve_conversation_id(&state, customer_id).await {
                Ok(cid) => cid,
                Err(e) => {
                    warn!(
                        "Failed to resolve conversation_id for PSID {}, using PSID as fallback: {e}",
                        customer_id
                    );
                    customer_id.to_string()
                }
            };

            // Handle three event types: message, postback, delivery receipt
            let text: Option<String>;
            let external_id: Option<String>;

            if let Some(ref postback) = messaging.postback {
                let title = postback.title.as_deref().unwrap_or(&postback.payload);
                let payload_text = if let Some(ref ref_data) = postback.referral {
                    format!(
                        "[Button: {title}] (payload: {}, ref: {}, source: {})",
                        postback.payload,
                        ref_data.ref_.as_deref().unwrap_or(""),
                        ref_data.source.as_deref().unwrap_or("")
                    )
                } else {
                    format!("[Button: {title}] (payload: {})", postback.payload)
                };
                text = Some(payload_text);
                external_id = Some(format!(
                    "pb_{customer_id}_{}",
                    chrono::Utc::now().timestamp_millis()
                ));
            } else if let Some(ref msg) = messaging.message {
                let msg_text = msg
                    .text
                    .clone()
                    .or_else(|| msg.quick_reply.as_ref().map(|q| q.payload.clone()));

                let has_attachments = msg
                    .attachments
                    .as_ref()
                    .map(|a| !a.is_empty())
                    .unwrap_or(false);

                let final_text = if let Some(t) = msg_text {
                    if has_attachments {
                        let att_md = format_webhook_attachments(msg.attachments.as_deref());
                        Some(if t.trim().is_empty() {
                            att_md
                        } else {
                            format!("{t}\n{att_md}")
                        })
                    } else {
                        Some(t)
                    }
                } else if has_attachments {
                    Some(format_webhook_attachments(msg.attachments.as_deref()))
                } else {
                    None
                };
                text = final_text;
                external_id = msg.mid.clone();
            } else {
                // Delivery receipt or other non-content event — skip
                continue;
            }

            if let Some(msg_text) = text {
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
                        message_text: Some(msg_text.clone()),
                        external_id: external_id.clone(),
                        created_at: chrono::Utc::now(),
                    };

                    let store_result = state.message_client.store_message(payload).await;
                    let msg_id = match store_result {
                        Ok(msg) => msg.id,
                        Err(e) => {
                            warn!("Failed to store message: {}", e);
                            continue;
                        }
                    };

                    if let Some(ref eid) = external_id {
                        if state.mattermost_client.is_posted(eid).await {
                            continue;
                        }
                    }

                    let display_name = customer.name.as_deref().unwrap_or(&conversation_id);
                    let mattermost_post_result = post_to_mattermost(
                        &state,
                        &conversation_id,
                        &msg_text,
                        external_id.as_deref(),
                        display_name,
                        direction,
                        Some(customer_id),
                    )
                    .await;

                    if let Some(ref eid) = external_id {
                        if mattermost_post_result.is_ok() {
                            state.mattermost_client.mark_posted(eid);
                        }
                    }

                    if let Ok(Some(channel_id)) = mattermost_post_result {
                        if let Err(e) = state.message_client.mark_synced(msg_id, &channel_id).await
                        {
                            warn!("Failed to mark message as synced: {}", e);
                        }
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
    display_name: &str,
    direction: &str,
    customer_platform_id: Option<&str>,
) -> Result<Option<String>, anyhow::Error> {
    let mm = &state.mattermost_client;
    let team_id = match mm.get_team_id().await {
        Ok(id) => id,
        Err(e) => {
            warn!(
                "Could not determine Mattermost team_id for conversation {}: {e}",
                conversation_id
            );
            return Ok(None);
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
                        let fb_token = state.config.facebook_page_access_token.clone();
                        let psid = psid.to_string();
                        let bot_uid = bot_user_id.clone();
                        let mm_clone = mm.clone();
                        tokio::spawn(async move {
                            if let Ok(picture) =
                                crate::graph_api::get_profile_picture(&psid, &fb_token).await
                            {
                                if !picture.data.is_silhouette {
                                    if let Err(e) = mm_clone
                                        .set_user_profile_image(&bot_uid, &picture.data.url)
                                        .await
                                    {
                                        warn!(
                                            "Failed to set profile picture for bot {}: {}",
                                            bot_uid, e
                                        );
                                    } else {
                                        info!("Set profile picture for bot {}", bot_uid);
                                    }
                                }
                            }
                        });

                        let root = if let Some(r) = root_id {
                            Some(r.to_string())
                        } else {
                            mm.get_root_id(conversation_id).await.ok().flatten()
                        };
                        match mm
                            .post_message_as_bot_with_override(
                                &channel_id,
                                text,
                                root.as_deref(),
                                None,
                                &bot_user_id,
                                &bot_token,
                                Some(display_name),
                                None,
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
        Ok(Some(channel_id))
    } else {
        Ok(None)
    }
}

/// Resolve a customer PSID to the Facebook Graph API conversation ID (t_xxx format).
///
/// Uses a two-level cache strategy:
/// 1. In-memory HashMap (fastest, within single process lifetime)
/// 2. Database `mattermost_cache` with kind="conversation" (survives restarts)
/// 3. On cache miss, calls `GET /{page-id}/conversations?user_id={PSID}&fields=id`
///
/// This ensures webhook messages land in the same Mattermost channel as
/// poller/import messages which already use the t_xxx conversation ID.
async fn resolve_conversation_id(state: &AppState, psid: &str) -> Result<String, anyhow::Error> {
    // Check in-memory cache first
    {
        let cache = state.conversation_id_cache.read().await;
        if let Some(cached) = cache.get(psid) {
            return Ok(cached.clone());
        }
    }

    // Check database cache
    if let Ok(Some(cid)) = db::load_single_mm_cache(&state.pool, "conversation", psid).await {
        let mut cache = state.conversation_id_cache.write().await;
        cache.insert(psid.to_string(), cid.clone());
        return Ok(cid);
    }

    // Cache miss: resolve via Graph API
    let client = reqwest::Client::new();
    let page_id = &state.config.facebook_page_id;
    let access_token = &state.config.facebook_page_access_token;
    let url = format!(
        "https://graph.facebook.com/v24.0/{page_id}/conversations?user_id={psid}&fields=id&access_token={access_token}&limit=1",
    );

    let resp = client
        .get(&url)
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
        .context("Failed to call Graph API for conversation resolution")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(anyhow::anyhow!(
            "Graph API conversation resolution failed {status}: {body}"
        ));
    }

    #[derive(Deserialize)]
    struct ConversationResponse {
        data: Vec<ConversationEntry>,
    }
    #[derive(Deserialize)]
    struct ConversationEntry {
        id: String,
    }

    let conv_resp: ConversationResponse = resp
        .json()
        .await
        .context("Failed to parse conversation resolution response")?;

    let conversation_id = conv_resp
        .data
        .first()
        .map(|c| c.id.clone())
        .ok_or_else(|| anyhow::anyhow!("No conversation found for PSID {psid}"))?;

    state
        .conversation_id_cache
        .write()
        .await
        .insert(psid.to_string(), conversation_id.clone());
    if let Err(e) = db::upsert_mm_cache(&state.pool, "conversation", psid, &conversation_id).await {
        warn!(
            "Failed to persist conversation cache for PSID {}: {e}",
            psid
        );
    }

    info!(
        "Resolved PSID {} → conversation_id {} via Graph API",
        psid, conversation_id
    );

    Ok(conversation_id)
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
        let url = att.payload.as_ref().and_then(|p| p.url.as_deref());
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
    /// Facebook postback events (persistent menu buttons, Get Started, etc.)
    /// These fire when a user clicks a button rather than typing a message.
    pub postback: Option<WebhookPostback>,
}

/// A postback payload from Facebook Messenger.
/// Triggered when users click persistent menu items, Get Started button,
/// or any button with a payload (not a URL button).
#[derive(Debug, Deserialize)]
pub struct WebhookPostback {
    /// The display title of the button the user clicked (e.g. "Menu của Bump")
    pub title: Option<String>,
    /// The developer-defined payload string (e.g. "MENU", "ADDRESS")
    pub payload: String,
    /// Referral data that may accompany the postback
    pub referral: Option<WebhookReferral>,
}

/// Referral data within a postback event
#[derive(Debug, Deserialize)]
pub struct WebhookReferral {
    /// The ref parameter (e.g. "MENU")
    pub ref_: Option<String>,
    /// The source of the referral (e.g. "SHORTLINK", "MESSENGER_CODE")
    pub source: Option<String>,
    /// The type of referral
    #[serde(rename = "type")]
    pub referral_type: Option<String>,
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

        // Pre-populate PSID→conversation_id cache so webhook handler can find it later
        {
            let cache = state.conversation_id_cache.read().await;
            if !cache.contains_key(&cust_id) {
                drop(cache);
                let mut cache = state.conversation_id_cache.write().await;
                if !cache.contains_key(&cust_id) {
                    cache.insert(cust_id.clone(), conversation_id.to_string());
                    let _ = crate::db::upsert_mm_cache(
                        &state.pool,
                        "conversation",
                        &cust_id,
                        conversation_id,
                    )
                    .await;
                }
            }
        }

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
                if !state
                    .mattermost_client
                    .mark_posted_persistent(&msg.id, conversation_id, &msg.id)
                    .await
                {
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
                                            state,
                                            mm,
                                            &channel_id,
                                            msg_text,
                                            &attachments,
                                            &msg.id,
                                            Some(msg_resp.id),
                                            Some(&bot_token),
                                        )
                                        .await
                                    } else {
                                        (msg_text.to_string(), Vec::new())
                                    };

                                    let result = if file_ids.is_empty() {
                                        mm.post_message_as_bot_with_override(
                                            &channel_id,
                                            &final_text,
                                            root_id_slice,
                                            ts,
                                            &bot_uid,
                                            &bot_token,
                                            Some(display_name),
                                            None,
                                        )
                                        .await
                                    } else {
                                        mm.post_message_as_bot_with_files_and_override(
                                            &channel_id,
                                            &final_text,
                                            root_id_slice,
                                            ts,
                                            &bot_uid,
                                            &bot_token,
                                            &file_ids,
                                            Some(display_name),
                                            None,
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
                                    state,
                                    mm,
                                    &channel_id,
                                    msg_text,
                                    &attachments,
                                    &msg.id,
                                    Some(msg_resp.id),
                                    None,
                                )
                                .await
                            } else {
                                (msg_text.to_string(), Vec::new())
                            };

                            match mm
                                .post_message_with_files(
                                    &channel_id,
                                    &final_text,
                                    root_id_slice,
                                    ts,
                                    &file_ids,
                                )
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

    mm.clear_root_id(&conversation_id);
    mm.clear_root_id_db(&state.pool, &conversation_id);
    let _ = crate::db::clear_posted_messages(&state.pool, &conversation_id).await;

    // Delete all existing posts in the channel
    let deleted = mm
        .delete_all_posts_in_channel(&channel_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

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
                    .post_message_as_bot_with_override(
                        &channel_id,
                        text,
                        root,
                        None,
                        &bot_uid,
                        &bot_token,
                        Some(customer_name),
                        None,
                    )
                    .await
                {
                    Ok(post_id) => {
                        if root_id.is_none() {
                            mm.set_root_id(&conversation_id, &post_id);
                            root_id = Some(post_id.clone());
                        }
                        mm.mark_posted_persistent(&msg.id, &conversation_id, &post_id)
                            .await;
                        posted += 1;
                    }
                    Err(e) => {
                        tracing::warn!("Reimport: bot post failed for {}: {}", conversation_id, e);
                        let root = root_id.as_deref();
                        if let Ok(post_id) = mm.post_message(&channel_id, text, root, None).await {
                            if root_id.is_none() {
                                mm.set_root_id(&conversation_id, &post_id);
                                root_id = Some(post_id.clone());
                            }
                            mm.mark_posted_persistent(&msg.id, &conversation_id, &post_id)
                                .await;
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
                        root_id = Some(post_id.clone());
                    }
                    mm.mark_posted_persistent(&msg.id, &conversation_id, &post_id)
                        .await;
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
                root_id = Some(post_id.clone());
            }
            mm.mark_posted_persistent(&msg.id, &conversation_id, &post_id)
                .await;
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

    mm.clear_root_id(conversation_id);
    mm.clear_root_id_db(&state.pool, conversation_id);
    let _ = crate::db::clear_posted_messages(&state.pool, conversation_id).await;

    let deleted = mm.delete_all_posts_in_channel(channel_id).await?;

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
                        state,
                        mm,
                        channel_id,
                        text,
                        &attachments,
                        &msg.id,
                        None,
                        Some(&bot_token),
                    )
                    .await
                } else {
                    (text.to_string(), Vec::new())
                };

                let root = root_id.as_deref();
                let result = if file_ids.is_empty() {
                    mm.post_message_as_bot_with_override(
                        channel_id,
                        &final_text,
                        root,
                        None,
                        &bot_uid,
                        &bot_token,
                        Some(customer_name),
                        None,
                    )
                    .await
                } else {
                    mm.post_message_as_bot_with_files_and_override(
                        channel_id,
                        &final_text,
                        root,
                        None,
                        &bot_uid,
                        &bot_token,
                        &file_ids,
                        Some(customer_name),
                        None,
                    )
                    .await
                };
                match result {
                    Ok(post_id) => {
                        if root_id.is_none() {
                            mm.set_root_id(conversation_id, &post_id);
                            root_id = Some(post_id.clone());
                        }
                        if mm
                            .mark_posted_persistent(&msg.id, conversation_id, &post_id)
                            .await
                        {
                            posted += 1;
                        }
                    }
                    Err(e) => {
                        tracing::warn!("Reimport: bot post failed for {}: {}", conversation_id, e);
                        let root = root_id.as_deref();
                        if let Ok(post_id) =
                            mm.post_message(channel_id, &final_text, root, None).await
                        {
                            if root_id.is_none() {
                                mm.set_root_id(conversation_id, &post_id);
                                root_id = Some(post_id.clone());
                            }
                            if mm
                                .mark_posted_persistent(&msg.id, conversation_id, &post_id)
                                .await
                            {
                                posted += 1;
                            }
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
                        root_id = Some(post_id.clone());
                    }
                    if mm
                        .mark_posted_persistent(&msg.id, conversation_id, &post_id)
                        .await
                    {
                        posted += 1;
                    }
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
                state,
                mm,
                channel_id,
                text,
                &attachments,
                &msg.id,
                None,
                None,
            )
            .await
        } else {
            (text.to_string(), Vec::new())
        };

        let root = root_id.as_deref();
        let result = if file_ids.is_empty() {
            mm.post_message(channel_id, &final_text, root, None).await
        } else {
            mm.post_message_with_files(channel_id, &final_text, root, None, &file_ids)
                .await
        };
        if let Ok(post_id) = result {
            if root_id.is_none() {
                mm.set_root_id(conversation_id, &post_id);
                root_id = Some(post_id.clone());
            }
            if mm
                .mark_posted_persistent(&msg.id, conversation_id, &post_id)
                .await
            {
                posted += 1;
            }
        }
    }

    Ok(ReimportResult {
        deleted_posts: deleted,
        messages_fetched: total,
        messages_posted: posted,
    })
}

pub async fn full_history_reimport(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    info!("Starting full history reimport from Facebook Graph API");

    // Spawn background task to avoid HTTP timeout
    tokio::spawn(async move {
        let result = full_history_reimport_task(&state).await;
        match result {
            Ok(summary) => {
                info!(
                    "Full history reimport complete: {} conversations processed, {} posts deleted, {} messages fetched, {} messages posted, {} errors",
                    summary.conversations_processed,
                    summary.posts_deleted,
                    summary.messages_fetched,
                    summary.messages_posted,
                    summary.errors
                );
            }
            Err(e) => {
                error!("Full history reimport failed: {}", e);
            }
        }
    });

    Ok(Json(serde_json::json!({
        "status": "started",
        "message": "Full history reimport running in background. Check logs for progress."
    })))
}

struct FullHistorySummary {
    conversations_processed: u32,
    posts_deleted: u32,
    messages_fetched: usize,
    messages_posted: u32,
    errors: u32,
}

async fn full_history_reimport_task(state: &AppState) -> Result<FullHistorySummary, anyhow::Error> {
    let mm = &state.mattermost_client;
    let team_id = mm.get_team_id().await?;
    let start_time = Instant::now();
    let max_duration = Duration::from_secs(3600 * 2); // 2 hour max

    let conversations = graph_api::get_conversations(&state.pool, &state.config).await?;
    let total_conversations = conversations.len();
    info!(
        "Full history reimport: found {} total conversations on Facebook",
        total_conversations
    );

    let mut summary = FullHistorySummary {
        conversations_processed: 0,
        posts_deleted: 0,
        messages_fetched: 0,
        messages_posted: 0,
        errors: 0,
    };

    for conv in &conversations {
        // Check timeout - stop after 2 hours to avoid blocking forever
        if start_time.elapsed() > max_duration {
            warn!(
                "Full history reimport timed out after 2 hours: {} conversations processed, {} messages posted",
                summary.conversations_processed,
                summary.messages_posted
            );
            break;
        }

        let conv_id = &conv.id;

        if !conv_id.starts_with("t_") {
            continue;
        }

        let messages = match graph_api::get_conversation_messages(
            &state.pool,
            conv_id,
            &state.config.facebook_page_access_token,
        )
        .await
        {
            Ok(msgs) => msgs,
            Err(e) => {
                warn!(
                    "Full history reimport: failed to fetch messages for {}: {}",
                    conv_id, e
                );
                summary.errors += 1;
                continue;
            }
        };

        if messages.is_empty() {
            continue;
        }

        summary.messages_fetched += messages.len();

        let mut messages_sorted = messages;
        messages_sorted.sort_by_key(|m| m.created_time);

        let display_name = messages_sorted
            .iter()
            .find_map(|msg| {
                if msg.from.id != state.config.facebook_page_id {
                    Some(msg.from.name.clone())
                } else {
                    msg.to.data.first().map(|u| u.name.clone())
                }
            })
            .unwrap_or_else(|| conv_id.clone());

        let channel_id = match mm
            .get_or_create_channel(&team_id, conv_id, &display_name)
            .await
        {
            Ok(cid) => cid,
            Err(e) => {
                warn!(
                    "Full history reimport: failed to create channel for {}: {}",
                    conv_id, e
                );
                summary.errors += 1;
                continue;
            }
        };

        let _ = mm
            .maybe_update_display_name(&channel_id, conv_id, &display_name)
            .await;

        mm.clear_root_id(conv_id);
        mm.clear_root_id_db(&state.pool, conv_id);
        let _ = crate::db::clear_posted_messages(&state.pool, conv_id).await;

        let deleted_this_channel = mm
            .delete_all_posts_in_channel(&channel_id)
            .await
            .unwrap_or(0);
        if deleted_this_channel > 0 {
            info!(
                "Full history reimport: deleted {} posts from channel {}",
                deleted_this_channel, conv_id
            );
            summary.posts_deleted += deleted_this_channel;
        }

        {
            let mut cache = state.conversation_id_cache.write().await;
            for msg in &messages_sorted {
                if msg.from.id != state.config.facebook_page_id {
                    cache.insert(msg.from.id.clone(), conv_id.clone());
                }
            }
        }

        let mut root_id: Option<String> = None;

        for msg in &messages_sorted {
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

            let bot_result = mm
                .get_or_create_customer_bot(cust_id, customer_name, &channel_id)
                .await;

            let (bot_uid, bot_token) = match bot_result {
                Ok((uid, tok)) => (uid, tok),
                Err(e) => {
                    warn!(
                        "Full history reimport: bot creation failed for {}: {}",
                        cust_id, e
                    );
                    continue;
                }
            };

            let (final_text, file_ids) = if !attachments.is_empty() {
                crate::media::process_attachments_for_post(
                    state,
                    mm,
                    &channel_id,
                    text,
                    &attachments,
                    &msg.id,
                    None,
                    Some(&bot_token),
                )
                .await
            } else {
                (text.to_string(), Vec::new())
            };

            let root = root_id.as_deref();
            let msg_ts = Some(msg.created_time.timestamp_millis());
            let post_result = if file_ids.is_empty() {
                mm.post_message_as_bot_with_override(
                    &channel_id,
                    &final_text,
                    root,
                    msg_ts,
                    &bot_uid,
                    &bot_token,
                    Some(customer_name),
                    None,
                )
                .await
            } else {
                mm.post_message_as_bot_with_files_and_override(
                    &channel_id,
                    &final_text,
                    root,
                    msg_ts,
                    &bot_uid,
                    &bot_token,
                    &file_ids,
                    Some(customer_name),
                    None,
                )
                .await
            };

            match post_result {
                Ok(post_id) => {
                    if root_id.is_none() {
                        mm.set_root_id(conv_id, &post_id);
                        root_id = Some(post_id.clone());
                    }
                    if mm.mark_posted_persistent(&msg.id, conv_id, &post_id).await {
                        summary.messages_posted += 1;
                    }
                }
                Err(e) => {
                    warn!(
                        "Full history reimport: bot post failed for {}: {}",
                        conv_id, e
                    );
                }
            }
        }

        for msg in &messages_sorted {
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
                    state,
                    mm,
                    &channel_id,
                    text,
                    &attachments,
                    &msg.id,
                    None,
                    None,
                )
                .await
            } else {
                (text.to_string(), Vec::new())
            };

            let root = root_id.as_deref();
            let msg_ts = Some(msg.created_time.timestamp_millis());
            let post_result = if file_ids.is_empty() {
                mm.post_message(&channel_id, &final_text, root, msg_ts)
                    .await
            } else {
                mm.post_message_with_files(&channel_id, &final_text, root, msg_ts, &file_ids)
                    .await
            };

            if let Ok(post_id) = post_result {
                if root_id.is_none() {
                    mm.set_root_id(conv_id, &post_id);
                    root_id = Some(post_id.clone());
                }
                if mm.mark_posted_persistent(&msg.id, conv_id, &post_id).await {
                    summary.messages_posted += 1;
                }
            }
        }

        summary.conversations_processed += 1;

        // Log progress every 100 conversations
        if summary.conversations_processed.checked_rem(100) == Some(0) {
            info!(
                "Full history reimport progress: {}/{} conversations, {} posts deleted, {} messages posted",
                summary.conversations_processed,
                total_conversations,
                summary.posts_deleted,
                summary.messages_posted
            );
        }

        tokio::time::sleep(Duration::from_millis(500)).await;
    }

    Ok(summary)
}

#[derive(Clone)]
pub struct SyncResult {
    pub messages_fetched: usize,
    pub messages_posted: u32,
    pub messages_skipped: u32,
}

pub async fn sync_all_conversations(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let channels = state
        .mattermost_client
        .get_all_t_channels()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let total = channels.len();
    info!("Sync-all: starting background sync of {} channels", total);

    tokio::spawn(async move {
        let mut total_fetched = 0usize;
        let mut total_posted = 0u32;
        let mut total_skipped = 0u32;
        let mut errors = 0u32;

        for (idx, channel) in channels.iter().enumerate() {
            let conv_id = &channel.name;
            info!("Sync-all [{}/{}]: processing {}", idx + 1, total, conv_id);

            match sync_conversation(&state, conv_id).await {
                Ok(result) => {
                    total_fetched += result.messages_fetched;
                    total_posted += result.messages_posted;
                    total_skipped += result.messages_skipped;
                }
                Err(e) => {
                    tracing::error!("Sync-all: failed for {}: {}", conv_id, e);
                    errors += 1;
                }
            }
        }

        info!(
            "Sync-all complete: {} channels, {} fetched, {} posted, {} skipped, {} errors",
            total, total_fetched, total_posted, total_skipped, errors
        );
    });

    Ok(Json(serde_json::json!({
        "status": "started",
        "total_channels": total,
        "message": "Sync running in background. Check logs for progress."
    })))
}

pub async fn sync_all_conversations_sync(state: &AppState) -> Result<SyncResult, anyhow::Error> {
    let channels = state.mattermost_client.get_all_t_channels().await?;
    let total = channels.len();
    info!("Sync-all (sync): starting sync of {} channels", total);

    let mut total_fetched = 0usize;
    let mut total_posted = 0u32;
    let mut total_skipped = 0u32;
    let mut errors = 0u32;

    for (idx, channel) in channels.iter().enumerate() {
        let conv_id = &channel.name;
        info!("Sync-all [{}/{}]: processing {}", idx + 1, total, conv_id);

        match sync_conversation(state, conv_id).await {
            Ok(result) => {
                total_fetched += result.messages_fetched;
                total_posted += result.messages_posted;
                total_skipped += result.messages_skipped;
            }
            Err(e) => {
                tracing::error!("Sync-all: failed for {}: {}", conv_id, e);
                errors += 1;
            }
        }
    }

    info!(
        "Sync-all complete: {} channels, {} fetched, {} posted, {} skipped, {} errors",
        total, total_fetched, total_posted, total_skipped, errors
    );

    Ok(SyncResult {
        messages_fetched: total_fetched,
        messages_posted: total_posted,
        messages_skipped: total_skipped,
    })
}

async fn sync_conversation(
    state: &AppState,
    conversation_id: &str,
) -> Result<SyncResult, anyhow::Error> {
    let mm = &state.mattermost_client;

    let messages = graph_api::get_conversation_messages(
        &state.pool,
        conversation_id,
        &state.config.facebook_page_access_token,
    )
    .await
    .map_err(|e| anyhow::anyhow!("Failed to fetch messages: {e}"))?;

    let total = messages.len();
    tracing::info!(
        "Sync: fetched {} messages for conversation {}",
        total,
        conversation_id
    );

    let mut messages = messages;
    messages.sort_by_key(|m| m.created_time);

    let mut timestamp_counts: std::collections::HashMap<i64, u32> =
        std::collections::HashMap::new();
    for msg in &mut messages {
        let ts = msg.created_time.timestamp_millis();
        let count = timestamp_counts.entry(ts).or_insert(0);
        if *count > 0 {
            let offset = *count as i64;
            msg.created_time =
                chrono::DateTime::from_timestamp_millis(ts + offset).unwrap_or(msg.created_time);
        }
        *count += 1;
    }

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
        .maybe_update_display_name_by_conversation_id(conversation_id, &display_name)
        .await;

    let mut posted = 0u32;
    let mut skipped = 0u32;
    let mut root_id: Option<String> = None;

    for msg in &messages {
        if msg.from.id == state.config.facebook_page_id {
            continue;
        }

        if mm.is_posted(&msg.id).await {
            skipped += 1;
            continue;
        }

        let text = match &msg.message {
            Some(t) if !t.trim().is_empty() => t.as_str(),
            _ => continue,
        };

        let customer_name = &msg.from.name;
        let cust_id = &msg.from.id;

        let (bot_uid, bot_token) = match mm
            .get_or_create_customer_bot(cust_id, customer_name, conversation_id)
            .await
        {
            Ok(bot) => bot,
            Err(e) => {
                tracing::warn!("Sync: bot creation failed for {}: {}", cust_id, e);
                continue;
            }
        };

        let team_id = match mm.get_team_id().await {
            Ok(tid) => tid,
            Err(e) => {
                tracing::warn!("Sync: failed to get team_id: {}", e);
                continue;
            }
        };
        let channel_id = match mm
            .get_or_create_channel(&team_id, conversation_id, display_name.as_str())
            .await
        {
            Ok(cid) => cid,
            Err(e) => {
                tracing::warn!("Sync: failed to get channel: {}", e);
                continue;
            }
        };

        let root = root_id.as_deref();
        let msg_ts = Some(msg.created_time.timestamp_millis());

        let attachments = crate::media::extract_attachments_from_graph(msg);
        let (msg_text, file_ids) = if !attachments.is_empty() {
            crate::media::process_attachments_for_post(
                state,
                mm,
                &channel_id,
                text,
                &attachments,
                &msg.id,
                None,
                Some(&bot_token),
            )
            .await
        } else {
            (text.to_string(), Vec::new())
        };

        match if file_ids.is_empty() {
            mm.post_message_as_bot_with_override(
                &channel_id,
                &msg_text,
                root,
                msg_ts,
                &bot_uid,
                &bot_token,
                Some(customer_name),
                None,
            )
            .await
        } else {
            mm.post_message_as_bot_with_files_and_override(
                &channel_id,
                &msg_text,
                root,
                msg_ts,
                &bot_uid,
                &bot_token,
                &file_ids,
                Some(customer_name),
                None,
            )
            .await
        } {
            Ok(post_id) => {
                if root_id.is_none() {
                    mm.set_root_id(conversation_id, &post_id);
                    root_id = Some(post_id);
                }
                mm.mark_posted(&msg.id);
                posted += 1;
            }
            Err(e) => {
                tracing::warn!(
                    "Sync: bot post failed for {}, falling back to admin: {}",
                    conversation_id,
                    e
                );
                let fallback_result = mm.post_message(&channel_id, &msg_text, root, msg_ts).await;
                match fallback_result {
                    Ok(post_id) => {
                        if root_id.is_none() {
                            mm.set_root_id(conversation_id, &post_id);
                            root_id = Some(post_id);
                        }
                        mm.mark_posted(&msg.id);
                        posted += 1;
                    }
                    Err(e2) => {
                        tracing::warn!(
                            "Sync: fallback to admin failed for {}: {}",
                            conversation_id,
                            e2
                        );
                        mm.mark_posted(&msg.id);
                    }
                }
            }
        }
    }

    Ok(SyncResult {
        messages_fetched: total,
        messages_posted: posted,
        messages_skipped: skipped,
    })
}

#[derive(Debug, Serialize)]
pub struct UpdateAvatarsResult {
    pub total: usize,
    pub updated: usize,
    pub failed: usize,
    pub skipped: usize,
    pub errors: Vec<String>,
}

pub async fn update_all_avatars(
    State(state): State<AppState>,
) -> Result<Json<UpdateAvatarsResult>, (StatusCode, String)> {
    let mm = &state.mattermost_client;
    let fb_token = &state.config.facebook_page_access_token;

    let bot_users = mm.get_all_bot_users().await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to get bot users: {e}"),
        )
    })?;

    let mut updated = 0;
    let mut failed = 0;
    let mut skipped = 0;
    let mut errors = Vec::new();

    info!("Updating avatars for {} bot users", bot_users.len());

    let psids: Vec<String> = bot_users
        .iter()
        .map(|bot| {
            bot.username
                .strip_prefix("fb-")
                .unwrap_or(&bot.username)
                .to_string()
        })
        .collect();

    info!(
        "Fetching profile pictures for {} PSIDs in batch",
        psids.len()
    );

    match graph_api::get_profile_pictures_batch(&psids, fb_token).await {
        Ok(results) => {
            info!("Got batch profile picture results: {} PSIDs", results.len());

            for bot in &bot_users {
                let psid = bot.username.strip_prefix("fb-").unwrap_or(&bot.username);

                match results.get(psid) {
                    Some(Ok(picture_data)) => {
                        if picture_data.is_silhouette {
                            info!(
                                "Bot {} has no profile picture (silhouette), skipping",
                                bot.username
                            );
                            skipped += 1;
                            continue;
                        }

                        info!(
                            "Setting avatar for bot {} from URL: {}",
                            bot.username, picture_data.url
                        );

                        match mm.set_user_profile_image(&bot.id, &picture_data.url).await {
                            Ok(()) => {
                                info!("Updated avatar for bot {} ({})", bot.username, bot.id);
                                updated += 1;
                            }
                            Err(e) => {
                                warn!("Failed to set avatar for bot {}: {}", bot.username, e);
                                failed += 1;
                                errors.push(format!("{}: set avatar failed: {e}", bot.username));
                            }
                        }
                    }
                    Some(Err(e)) => {
                        warn!(
                            "Failed to get profile picture for bot {}: {}",
                            bot.username, e
                        );
                        failed += 1;
                        errors.push(format!("{}: {e}", bot.username));
                    }
                    None => {
                        warn!(
                            "No profile picture result for bot {} (PSID: {})",
                            bot.username, psid
                        );
                        failed += 1;
                        errors.push(format!("{}: no result returned", bot.username));
                    }
                }

                tokio::time::sleep(Duration::from_millis(200)).await;
            }
        }
        Err(e) => {
            warn!("Batch profile picture fetch failed: {}", e);
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Batch fetch failed: {e}"),
            ));
        }
    }

    info!(
        "Avatar update complete: total={}, updated={}, failed={}, skipped={}",
        bot_users.len(),
        updated,
        failed,
        skipped
    );

    Ok(Json(UpdateAvatarsResult {
        total: bot_users.len(),
        updated,
        failed,
        skipped,
        errors,
    }))
}
