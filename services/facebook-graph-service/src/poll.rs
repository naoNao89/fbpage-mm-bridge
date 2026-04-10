use crate::AppState;
use std::time::Duration;
use tracing::{error, info, warn};

pub async fn run_poller(state: AppState, interval_secs: u64) {
    let mut last_poll_ts = chrono::Utc::now() - chrono::Duration::seconds(interval_secs as i64 * 2);

    loop {
        tokio::time::sleep(Duration::from_secs(interval_secs)).await;

        let since = last_poll_ts - chrono::Duration::seconds(30);
        info!("Poller: checking for conversations updated after {}", since);

        match poll_recent_conversations(&state, since).await {
            Ok(count) => {
                info!("Poller: posted {} new messages to Mattermost", count);
                last_poll_ts = chrono::Utc::now();
            }
            Err(e) => {
                error!("Poller error: {}", e);
                tokio::time::sleep(Duration::from_secs(10)).await;
            }
        }
    }
}

async fn poll_recent_conversations(
    state: &AppState,
    since: chrono::DateTime<chrono::Utc>,
) -> anyhow::Result<usize> {
    let conversations = crate::graph_api::get_recent_conversations(&state.config, since).await?;

    if conversations.is_empty() {
        return Ok(0);
    }

    info!(
        "Poller: {} conversations updated since {}",
        conversations.len(),
        since
    );

    let mut total_posted = 0;

    for conv in &conversations {
        match poll_conversation_new_messages(state, &conv.id, since).await {
            Ok(count) => {
                if count > 0 {
                    info!(
                        "Poller: posted {} new messages from conversation {}",
                        count, conv.id
                    );
                }
                total_posted += count;
            }
            Err(e) => {
                warn!("Poller: failed to process conversation {}: {}", conv.id, e);
            }
        }

        tokio::time::sleep(Duration::from_millis(200)).await;
    }

    Ok(total_posted)
}

async fn poll_conversation_new_messages(
    state: &AppState,
    conversation_id: &str,
    since: chrono::DateTime<chrono::Utc>,
) -> anyhow::Result<usize> {
    let messages = crate::graph_api::get_conversation_messages_since(
        conversation_id,
        &state.config.facebook_page_access_token,
        since,
    )
    .await?;

    if messages.is_empty() {
        return Ok(0);
    }

    let mm = &state.mattermost_client;
    let team_id = mm.get_team_id().await?;

    let first_customer_name = messages.iter().find_map(|msg| {
        let is_from_page = msg.from.id == state.config.facebook_page_id;
        if is_from_page {
            msg.to.data.first().map(|u| u.name.clone())
        } else {
            Some(msg.from.name.clone())
        }
    });

    let display_name = first_customer_name.as_deref().unwrap_or(conversation_id);
    let channel_id = mm
        .get_or_create_channel(&team_id, conversation_id, display_name)
        .await?;

    let _ = mm
        .maybe_update_display_name(&channel_id, conversation_id, display_name)
        .await;

    let root_id = mm.get_root_id(conversation_id).await?;

    let mut posted = 0;

    for msg in &messages {
        let is_from_page = msg.from.id == state.config.facebook_page_id;
        let direction = if is_from_page { "outgoing" } else { "incoming" };

        let (customer_platform_id, customer_name) = if is_from_page {
            (
                msg.to.data.first().map(|u| u.id.clone()),
                msg.to.data.first().map(|u| u.name.clone()),
            )
        } else {
            (Some(msg.from.id.clone()), Some(msg.from.name.clone()))
        };

        let cust_id = match customer_platform_id {
            Some(id) => id,
            None => {
                warn!("Skipping message {}: no customer ID", msg.id);
                continue;
            }
        };

        let customer = match state
            .customer_client
            .get_or_create_customer(&cust_id, "facebook", customer_name.as_deref())
            .await
        {
            Ok(c) => c,
            Err(e) => {
                warn!("Failed to get/create customer {}: {}", cust_id, e);
                continue;
            }
        };

        let message_payload = crate::services::MessageServicePayload {
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
                let text = msg.message.as_deref().unwrap_or("");
                let msg_root = root_id.as_deref();
                match mm
                    .post_message(
                        &channel_id,
                        text,
                        msg_root,
                        Some(msg.created_time.timestamp_millis()),
                    )
                    .await
                {
                    Ok(post_id) => {
                        if root_id.is_none() {
                            mm.set_root_id(conversation_id, &post_id);
                        }
                        posted += 1;
                    }
                    Err(e) if e.to_string().contains("Duplicate post skipped") => {}
                    Err(e) if e.to_string().contains("Skipping empty message") => {}
                    Err(e) => {
                        warn!("Mattermost post failed for {}: {}", conversation_id, e);
                    }
                }
            }
            Err(e) if e.to_string().contains("already exists") => {}
            Err(e) => {
                warn!("Failed to store message {}: {}", msg.id, e);
            }
        }
    }

    Ok(posted)
}
