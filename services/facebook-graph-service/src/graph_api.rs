//! Facebook Graph API client
//!
//! Handles all communication with Facebook Graph API including:
//! - Fetching conversations (page inbox)
//! - Fetching messages
//! - Token validation
//! - Rate limit tracking

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use reqwest::Client;
use serde::Deserialize;
use sqlx::PgPool;
use std::time::Duration;
use tracing::{debug, info, warn};

use crate::config::Config;
use crate::db;
use crate::models::{
    Conversation, ConversationsResponse, FacebookRateLimitInfo, GraphMessage, MessagesResponse,
};

/// Facebook Graph API base URL
const GRAPH_API_BASE: &str = "https://graph.facebook.com/v24.0";

/// Default request timeout
const REQUEST_TIMEOUT_SECS: u64 = 30;

/// Delay between pagination requests (ms)
const PAGINATION_DELAY_MS: u64 = 500;

// Token Debugging

#[derive(Debug, Deserialize)]
struct DebugTokenData {
    #[serde(rename = "type")]
    token_type: String,
}

#[derive(Debug, Deserialize)]
struct DebugTokenResponse {
    data: DebugTokenData,
}

/// Detect the type of a Facebook access token (USER or PAGE)
async fn detect_token_type(access_token: &str, app_id: &str, app_secret: &str) -> Result<String> {
    let client = Client::new();
    let app_access_token = format!("{app_id}|{app_secret}");
    let url = format!(
        "{GRAPH_API_BASE}/debug_token?input_token={access_token}&access_token={app_access_token}"
    );

    let response = client
        .get(&url)
        .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS))
        .send()
        .await
        .context("Failed to send debug_token request")?;

    if !response.status().is_success() {
        let status = response.status();
        let error_text = response.text().await?;
        return Err(anyhow::anyhow!(
            "debug_token failed: {status} - {error_text}"
        ));
    }

    let debug_response: DebugTokenResponse = response
        .json()
        .await
        .context("Failed to parse debug_token response")?;

    info!("Detected token type: {}", debug_response.data.token_type);
    Ok(debug_response.data.token_type)
}

// URL Builders

/// Build URL for fetching conversations
fn build_conversations_url(page_id: &str, access_token: &str) -> String {
    format!(
        "{GRAPH_API_BASE}/{page_id}/conversations?fields=id,updated_time,message_count&access_token={access_token}&limit=100"
    )
}

/// Build URL for fetching messages from a conversation
fn build_messages_url(conversation_id: &str, access_token: &str) -> String {
    format!(
        "{GRAPH_API_BASE}/{conversation_id}/messages?fields=id,created_time,from,message,to,attachments{{id,name,mime_type,size,image_data{{url}},video_data{{url}},file_url}}&access_token={access_token}&limit=100"
    )
}

// Rate Limit Handling

/// Extract rate limit info from Facebook API response headers
pub fn extract_rate_limit_from_response(
    response: &reqwest::Response,
) -> Option<FacebookRateLimitInfo> {
    // Try X-App-Usage header first
    if let Some(app_usage) = response.headers().get("x-app-usage") {
        if let Ok(usage_str) = app_usage.to_str() {
            if let Ok(usage_json) = serde_json::from_str::<serde_json::Value>(usage_str) {
                return Some(FacebookRateLimitInfo {
                    call_count: usage_json["call_count"].as_i64().map(|v| v as i32),
                    total_cputime: usage_json["total_cputime"].as_i64().map(|v| v as i32),
                    total_time: usage_json["total_time"].as_i64().map(|v| v as i32),
                    call_count_limit: usage_json["call_count_limit"].as_i64().map(|v| v as i32),
                    cputime_limit: usage_json["total_cputime_limit"].as_i64().map(|v| v as i32),
                    time_limit: usage_json["total_time_limit"].as_i64().map(|v| v as i32),
                });
            }
        }
    }

    // Try X-Business-Use-Case-Usage header
    if let Some(business_usage) = response.headers().get("x-business-use-case-usage") {
        if let Ok(usage_str) = business_usage.to_str() {
            if let Ok(usage_json) = serde_json::from_str::<serde_json::Value>(usage_str) {
                if let Some((_key, usage_data)) = usage_json.as_object()?.iter().next() {
                    return Some(FacebookRateLimitInfo {
                        call_count: usage_data[0]["call_count"].as_i64().map(|v| v as i32),
                        total_cputime: usage_data[0]["total_cputime"].as_i64().map(|v| v as i32),
                        total_time: usage_data[0]["total_time"].as_i64().map(|v| v as i32),
                        call_count_limit: None,
                        cputime_limit: None,
                        time_limit: None,
                    });
                }
            }
        }
    }

    None
}

/// Store rate limit info in database
pub async fn store_rate_limit(
    pool: &PgPool,
    endpoint: &str,
    rate_limit: &FacebookRateLimitInfo,
) -> Result<()> {
    let reset_at = Utc::now() + chrono::Duration::minutes(60);

    let headers_json = serde_json::to_value(rate_limit)
        .map_err(|e| anyhow::anyhow!("Failed to serialize rate limit: {e}"))?;

    db::upsert_rate_limit(
        pool,
        endpoint,
        rate_limit.call_count.unwrap_or(0),
        rate_limit.call_count_limit.unwrap_or(0),
        reset_at,
        headers_json,
    )
    .await?;

    debug!(
        "Stored rate limit for {}: {} calls",
        endpoint,
        rate_limit.call_count.unwrap_or(0)
    );

    Ok(())
}

/// Check if we should back off based on rate limit status
pub async fn check_rate_limit_status(pool: &PgPool, endpoint: &str) -> Result<RateLimitStatus> {
    let is_limited = db::is_rate_limited(pool, endpoint).await?;

    let rate_limit = sqlx::query_as::<_, crate::models::FacebookRateLimit>(
        "SELECT * FROM facebook_rate_limits WHERE endpoint = $1",
    )
    .bind(endpoint)
    .fetch_optional(pool)
    .await
    .map_err(|e| anyhow::anyhow!(e.to_string()))?;

    if let Some(limit) = rate_limit {
        let usage_percent = calculate_usage_percent(&limit);
        let should_backoff = usage_percent >= 80.0;

        if usage_percent >= 95.0 {
            warn!(
                "Rate limit critical for {}: {:.1}% used",
                endpoint, usage_percent
            );
        }

        Ok(RateLimitStatus {
            is_limited,
            usage_percent,
            reset_at: limit
                .reset_at
                .unwrap_or_else(|| Utc::now() + chrono::Duration::hours(1)),
            should_backoff,
        })
    } else {
        Ok(RateLimitStatus {
            is_limited: false,
            usage_percent: 0.0,
            reset_at: Utc::now() + chrono::Duration::hours(1),
            should_backoff: false,
        })
    }
}

#[derive(Debug)]
pub struct RateLimitStatus {
    pub is_limited: bool,
    pub usage_percent: f32,
    pub reset_at: DateTime<Utc>,
    pub should_backoff: bool,
}

fn calculate_usage_percent(limit: &crate::models::FacebookRateLimit) -> f32 {
    if let Some(headers) = limit
        .last_response_headers
        .as_ref()
        .and_then(|v| v.as_object())
    {
        if let (Some(call_count), Some(call_limit)) = (
            headers.get("call_count").and_then(|v| v.as_i64()),
            headers.get("call_count_limit").and_then(|v| v.as_i64()),
        ) {
            if call_limit > 0 {
                return (call_count as f32 / call_limit as f32) * 100.0;
            }
        }
    }

    if let (Some(remaining), Some(total)) = (limit.calls_remaining, limit.calls_total) {
        if total > 0 {
            let used = total - remaining;
            return (used as f32 / total as f32) * 100.0;
        }
    }

    0.0
}

/// Calculate backoff duration based on usage percentage
pub fn calculate_backoff_duration(usage_percent: f32) -> Duration {
    if usage_percent >= 95.0 {
        Duration::from_secs(30 * 60)
    } else if usage_percent >= 80.0 {
        Duration::from_secs(10 * 60)
    } else {
        Duration::from_secs(0)
    }
}

// Efficient Polling - fetch only recently updated conversations

/// Fetch conversations updated after the given timestamp.
///
/// The FB Conversations API returns results sorted by `updated_time` descending,
/// so we fetch pages until we find conversations older than `since` — then stop.
/// This is O(recent_conversations) instead of O(all_conversations).
pub async fn get_recent_conversations(
    config: &Config,
    since: DateTime<Utc>,
) -> Result<Vec<Conversation>> {
    let client = Client::new();
    let access_token = &config.facebook_page_access_token;
    let page_id = &config.facebook_page_id;

    let mut recent = Vec::new();
    let mut next_url = Some(format!(
        "{GRAPH_API_BASE}/{page_id}/conversations?fields=id,updated_time,message_count&access_token={access_token}&limit=25"
    ));
    let mut page = 0;

    while let Some(url) = next_url {
        page += 1;
        let response = client
            .get(&url)
            .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS))
            .send()
            .await
            .context("Failed to fetch conversations for polling")?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await?;
            return Err(anyhow::anyhow!(
                "Conversations poll failed {status}: {error_text}"
            ));
        }

        let conv_response: ConversationsResponse = response
            .json()
            .await
            .context("Failed to parse conversations response")?;

        let mut found_old = false;
        for conv in conv_response.data {
            if conv.updated_time > since {
                recent.push(conv);
            } else {
                found_old = true;
                break;
            }
        }

        // Stop paginating once we see conversations older than our cutoff
        if found_old {
            break;
        }

        next_url = conv_response.paging.and_then(|p| p.next);

        if next_url.is_some() {
            tokio::time::sleep(Duration::from_millis(PAGINATION_DELAY_MS)).await;
        }
    }

    info!(
        "Poll: found {} conversations updated since {} ({} pages)",
        recent.len(),
        since,
        page
    );

    Ok(recent)
}

// Conversation Fetching

/// Fetch all conversations from Facebook Graph API with pagination
pub async fn get_conversations(pool: &PgPool, config: &Config) -> Result<Vec<Conversation>> {
    let client = Client::new();
    let access_token = &config.facebook_page_access_token;
    let page_id = &config.facebook_page_id;

    // Verify token type
    let token_type = detect_token_type(
        access_token,
        &config.facebook_app_id,
        &config.facebook_app_secret,
    )
    .await?;
    info!("Token type: {}", token_type);

    let mut all_conversations = Vec::new();
    let mut next_url = Some(build_conversations_url(page_id, access_token));
    let mut page_count = 0;

    info!("Starting to fetch conversations from Graph API");
    info!("Page ID: {}", page_id);

    while let Some(url) = next_url {
        page_count += 1;
        info!("Fetching conversations page {}", page_count);

        // Check rate limit status
        let rl_status = check_rate_limit_status(pool, "conversations").await?;
        if rl_status.should_backoff {
            let delay = calculate_backoff_duration(rl_status.usage_percent);
            warn!("Preemptive back-off for conversations: waiting {:?}", delay);
            tokio::time::sleep(delay).await;
        }

        let response = client
            .get(&url)
            .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS))
            .send()
            .await
            .context("Failed to fetch conversations")?;

        if let Some(info) = extract_rate_limit_from_response(&response) {
            let _ = store_rate_limit(pool, "conversations", &info).await;
        }

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await?;

            // Check for rate limit
            if status.as_u16() == 429 || error_text.contains("request limit reached") {
                return Err(anyhow::anyhow!("Facebook API rate limit reached"));
            }

            // Check for permission errors
            if error_text.contains("code\":100") && error_text.contains("missing permissions") {
                return Err(anyhow::anyhow!(
                    "Permission denied. The app may not have 'pages_messaging' permission. Error: {error_text}"
                ));
            }

            return Err(anyhow::anyhow!(
                "Graph API returned error {status}: {error_text}"
            ));
        }

        let conv_response: ConversationsResponse = response
            .json()
            .await
            .context("Failed to parse conversations response")?;

        let count = conv_response.data.len();
        all_conversations.extend(conv_response.data);

        info!("Fetched {} conversations on page {}", count, page_count);

        next_url = conv_response.paging.and_then(|p| p.next);

        if next_url.is_some() {
            tokio::time::sleep(Duration::from_millis(PAGINATION_DELAY_MS)).await;
        }
    }

    info!(
        "Completed fetching {} total conversations across {} pages",
        all_conversations.len(),
        page_count
    );

    Ok(all_conversations)
}

/// Fetch only messages created after the given timestamp for a conversation.
/// Stops paginating once all messages on a page are older than `since`,
/// because results come in reverse chronological order from FB.
pub async fn get_conversation_messages_since(
    conversation_id: &str,
    access_token: &str,
    since: DateTime<Utc>,
) -> Result<Vec<GraphMessage>> {
    let client = Client::new();
    let mut recent_messages = Vec::new();
    let mut next_url = Some(build_messages_url(conversation_id, access_token));
    let mut page = 0;

    while let Some(url) = next_url {
        page += 1;
        let response = client
            .get(&url)
            .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS))
            .send()
            .await
            .context("Failed to fetch messages for polling")?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await?;
            return Err(anyhow::anyhow!(
                "Messages poll failed for {conversation_id} ({status}): {error_text}"
            ));
        }

        let msg_response: MessagesResponse = response
            .json()
            .await
            .context("Failed to parse messages response")?;

        let mut found_old = false;
        for msg in &msg_response.data {
            if msg.created_time > since {
                recent_messages.push(msg.clone());
            } else {
                found_old = true;
                break;
            }
        }

        if found_old {
            break;
        }

        next_url = msg_response.paging.as_ref().and_then(|p| p.next.clone());

        if next_url.is_some() {
            tokio::time::sleep(Duration::from_millis(PAGINATION_DELAY_MS)).await;
        }
    }

    info!(
        "Poll: found {} new messages in conversation {} since {} ({} pages)",
        recent_messages.len(),
        conversation_id,
        since,
        page
    );

    Ok(recent_messages)
}

// Full Message Fetching (for historical import)

/// Fetch all messages for a conversation with pagination
pub async fn get_conversation_messages(
    pool: &PgPool,
    conversation_id: &str,
    access_token: &str,
) -> Result<Vec<GraphMessage>> {
    let client = Client::new();
    let mut all_messages = Vec::new();
    let mut next_url = Some(build_messages_url(conversation_id, access_token));
    let mut page_count = 0;

    while let Some(url) = next_url {
        page_count += 1;

        // Check rate limit status
        let rl_status = check_rate_limit_status(pool, "messages").await?;
        if rl_status.should_backoff {
            let delay = calculate_backoff_duration(rl_status.usage_percent);
            warn!(
                "Preemptive back-off for messages ({}): waiting {:?}",
                conversation_id, delay
            );
            tokio::time::sleep(delay).await;
        }

        let response = client
            .get(&url)
            .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS))
            .send()
            .await
            .context("Failed to fetch messages")?;

        if let Some(info) = extract_rate_limit_from_response(&response) {
            let _ = store_rate_limit(pool, "messages", &info).await;
        }

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await?;

            if status.as_u16() == 403 || error_text.contains("request limit reached") {
                return Err(anyhow::anyhow!(
                    "Rate limit exceeded for conversation {conversation_id}: {error_text}"
                ));
            }

            return Err(anyhow::anyhow!(
                "Graph API error for conversation {conversation_id} ({status}): {error_text}"
            ));
        }

        let msg_response: MessagesResponse = response
            .json()
            .await
            .context("Failed to parse messages response")?;

        all_messages.extend(msg_response.data);

        match &msg_response.paging {
            Some(paging) => {
                if paging.next.is_some() {
                    info!("Has next page for conversation {}", conversation_id);
                }
                next_url = paging.next.clone();
            }
            None => {
                next_url = None;
            }
        }

        if next_url.is_some() {
            tokio::time::sleep(Duration::from_millis(PAGINATION_DELAY_MS)).await;
        }
    }

    info!(
        "Fetched {} messages from conversation {} across {} pages",
        all_messages.len(),
        conversation_id,
        page_count
    );

    Ok(all_messages)
}

// Token Operations

/// Debug/verify an access token
pub async fn debug_token(access_token: &str, app_id: &str, app_secret: &str) -> Result<String> {
    detect_token_type(access_token, app_id, app_secret).await
}

/// Exchange a short-lived user access token for a long-lived token
///
/// This uses the Facebook OAuth endpoint to exchange a short-lived token
/// obtained from the Facebook Login flow for a long-lived token (60 days).
///
/// See: <https://developers.facebook.com/docs/facebook-login/guides/access-long-lived-tokens>
pub async fn exchange_token_for_long_lived(
    short_lived_token: &str,
    app_id: &str,
    app_secret: &str,
) -> Result<TokenExchangeResponse> {
    let client = Client::new();

    let url = format!(
        "{GRAPH_API_BASE}/oauth/access_token?\
            grant_type=fb_exchange_token\
            &client_id={app_id}\
            &client_secret={app_secret}\
            &fb_exchange_token={short_lived_token}"
    );

    info!("Exchanging short-lived token for long-lived token");

    let response = client
        .get(&url)
        .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS))
        .send()
        .await
        .context("Failed to send token exchange request")?;

    if !response.status().is_success() {
        let status = response.status();
        let error_text = response.text().await?;
        return Err(anyhow::anyhow!(
            "Token exchange failed: {status} - {error_text}"
        ));
    }

    let exchange_response: TokenExchangeResponse = response
        .json()
        .await
        .context("Failed to parse token exchange response")?;

    info!(
        "Successfully exchanged token. Token type: {}, expires in: {} seconds",
        exchange_response.token_type, exchange_response.expires_in
    );

    Ok(exchange_response)
}

/// Response from token exchange endpoint
#[derive(Debug, Deserialize)]
pub struct TokenExchangeResponse {
    /// The long-lived access token
    pub access_token: String,
    /// Token type (should be "bearer")
    #[serde(rename = "token_type")]
    pub token_type: String,
    /// Seconds until the token expires
    pub expires_in: i64,
}

// Send Message (Reply to Facebook)

/// Send a text message to a Facebook user via the Messenger Send API.
///
/// `recipient_psid` is the Page-Scoped User ID (the sender.id from webhook events).
/// Returns the message ID on success.
pub async fn send_message_to_facebook(
    access_token: &str,
    recipient_psid: &str,
    text: &str,
) -> Result<String> {
    let client = Client::new();
    let url = format!("{GRAPH_API_BASE}/me/messages?access_token={access_token}");

    let payload = serde_json::json!({
        "recipient": {
            "id": recipient_psid
        },
        "message": {
            "text": text
        }
    });

    let response = client
        .post(&url)
        .json(&payload)
        .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS))
        .send()
        .await
        .context("Failed to send message to Facebook")?;

    if !response.status().is_success() {
        let status = response.status();
        let error_text = response.text().await?;
        return Err(anyhow::anyhow!(
            "Facebook Send API failed with {status}: {error_text}"
        ));
    }

    let result: SendMessageResponse = response
        .json()
        .await
        .context("Failed to parse Send API response")?;

    info!(
        "Message sent to Facebook user {}: mid={}",
        recipient_psid, result.message_id
    );

    Ok(result.message_id)
}

/// Send an image attachment to a Facebook user via the Messenger Send API.
///
/// `image_url` must be a publicly accessible URL.
pub async fn send_image_to_facebook(
    access_token: &str,
    recipient_psid: &str,
    image_url: &str,
) -> Result<String> {
    let client = Client::new();
    let url = format!("{GRAPH_API_BASE}/me/messages?access_token={access_token}");

    let payload = serde_json::json!({
        "recipient": {
            "id": recipient_psid
        },
        "message": {
            "attachment": {
                "type": "image",
                "payload": {
                    "url": image_url
                }
            }
        }
    });

    let response = client
        .post(&url)
        .json(&payload)
        .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS))
        .send()
        .await
        .context("Failed to send image to Facebook")?;

    if !response.status().is_success() {
        let status = response.status();
        let error_text = response.text().await?;
        return Err(anyhow::anyhow!(
            "Facebook Send API (image) failed with {status}: {error_text}"
        ));
    }

    let result: SendMessageResponse = response
        .json()
        .await
        .context("Failed to parse Send API image response")?;

    info!(
        "Image sent to Facebook user {}: mid={}",
        recipient_psid, result.message_id
    );

    Ok(result.message_id)
}

#[derive(Debug, Deserialize)]
pub struct SendMessageResponse {
    #[serde(rename = "recipient_id")]
    pub recipient_id: String,
    #[serde(rename = "message_id")]
    pub message_id: String,
}

/// Resolve a Facebook user's real name from their Page-Scoped User ID.
///
/// Uses the Graph API `GET /{user-id}?fields=name` endpoint with the page
/// access token. Falls back to the raw `user_id` on any error so callers
/// always get a usable display string.
pub async fn resolve_facebook_user_name(
    access_token: &str,
    user_id: &str,
) -> Result<String> {
    let client = Client::new();
    let url = format!(
        "{GRAPH_API_BASE}/{user_id}?fields=name&access_token={access_token}"
    );

    let response = client
        .get(&url)
        .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS))
        .send()
        .await
        .context("Failed to fetch Facebook user profile")?;

    if !response.status().is_success() {
        let status = response.status();
        let error_text = response.text().await.unwrap_or_default();
        warn!(
            "Could not resolve Facebook user name for {} ({}): {}",
            user_id, status, error_text
        );
        // Fallback: raw ID so the channel still gets created
        return Ok(user_id.to_string());
    }

    #[derive(Debug, Deserialize)]
    struct UserProfile {
        name: Option<String>,
    }

    let profile: UserProfile = response
        .json()
        .await
        .context("Failed to parse Facebook user profile response")?;

    let name = profile
        .name
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| user_id.to_string());

    Ok(name)
}

// Tests

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_conversations_url() {
        let url = build_conversations_url("123456", "test_token");
        assert!(url.contains("123456/conversations"));
        assert!(url.contains("access_token=test_token"));
        assert!(url.contains("limit=100"));
    }

    #[test]
    fn test_build_messages_url() {
        let url = build_messages_url("conv_789", "test_token");
        assert!(url.contains("conv_789/messages"));
        assert!(url.contains("access_token=test_token"));
        assert!(url.contains("fields=id,created_time,from,message,to"));
    }
}
