use crate::AppState;
use std::time::Duration;
use tracing::{error, info, warn};
use uuid::Uuid;

async fn mark_message_synced(state: &AppState, msg_id: Uuid, channel_id: &str) {
    if let Err(e) = state.message_client.mark_synced(msg_id, channel_id).await {
        warn!(
            "Failed to mark message {} as synced to channel {}: {}",
            msg_id, channel_id, e
        );
    }
}

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
    let fb_conversations = crate::graph_api::get_recent_conversations(&state.config, since).await?;
    let ig_conversations =
        crate::graph_api::get_ig_recent_conversations(&state.config, since).await?;

    let all_conversations: Vec<_> = fb_conversations
        .into_iter()
        .chain(ig_conversations)
        .collect();

    if all_conversations.is_empty() {
        return Ok(0);
    }

    info!(
        "Poller: {} conversations updated since {}",
        all_conversations.len(),
        since
    );

    let mut total_posted = 0;

    for conv in &all_conversations {
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

    let mut incoming_msgs: Vec<&crate::models::GraphMessage> = Vec::new();
    let mut outgoing_msgs: Vec<&crate::models::GraphMessage> = Vec::new();

    for msg in &messages {
        let is_from_page = msg.from.id == state.config.facebook_page_id;
        if is_from_page {
            outgoing_msgs.push(msg);
        } else {
            incoming_msgs.push(msg);
        }
    }

    let ordered_msgs: Vec<&crate::models::GraphMessage> =
        incoming_msgs.into_iter().chain(outgoing_msgs).collect();

    for msg in ordered_msgs {
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
            Ok(msg_resp) => {
                if !mm
                    .mark_posted_persistent(&msg.id, conversation_id, &msg.id)
                    .await
                {
                    continue;
                }

                let text = msg.message.as_deref().unwrap_or("");
                let attachments = crate::media::extract_attachments_from_graph(msg);

                let msg_root = root_id.as_deref();
                let ts = Some(msg.created_time.timestamp_millis());
                let customer_name_str = customer.name.as_deref().unwrap_or(conversation_id);

                if direction == "incoming" {
                    match mm
                        .get_or_create_customer_bot(&cust_id, customer_name_str, &channel_id)
                        .await
                    {
                        Ok((bot_uid, bot_token)) => {
                            let (msg_text, file_ids) = if !attachments.is_empty() {
                                crate::media::process_attachments_for_post(
                                    state,
                                    mm,
                                    &channel_id,
                                    text,
                                    &attachments,
                                    &msg.id,
                                    Some(msg_resp.id),
                                    Some(&bot_token),
                                )
                                .await
                            } else {
                                (text.to_string(), Vec::new())
                            };

                            let result = if file_ids.is_empty() {
                                mm.post_message_as_bot_with_override(
                                    &channel_id,
                                    &msg_text,
                                    msg_root,
                                    ts,
                                    &bot_uid,
                                    &bot_token,
                                    Some(customer_name_str),
                                    None,
                                )
                                .await
                            } else {
                                mm.post_message_as_bot_with_files_and_override(
                                    &channel_id,
                                    &msg_text,
                                    msg_root,
                                    ts,
                                    &bot_uid,
                                    &bot_token,
                                    &file_ids,
                                    Some(customer_name_str),
                                    None,
                                )
                                .await
                            };
                            match result {
                                Ok(post_id) => {
                                    if root_id.is_none() {
                                        mm.set_root_id(conversation_id, &post_id);
                                    }
                                    mark_message_synced(state, msg_resp.id, &channel_id).await;
                                    posted += 1;
                                }
                                Err(e) if e.to_string().contains("Duplicate post skipped") => {}
                                Err(e) if e.to_string().contains("Skipping empty message") => {}
                                Err(e) if e.to_string().contains("Invalid RootId cleared") => {
                                    warn!(
                                        "Bot post failed with Invalid RootId for {}, retrying without root: {e}",
                                        conversation_id
                                    );
                                    let root = None;
                                    let retry_result = if file_ids.is_empty() {
                                        mm.post_message_as_bot_with_override(
                                            &channel_id,
                                            &msg_text,
                                            root,
                                            ts,
                                            &bot_uid,
                                            &bot_token,
                                            Some(customer_name_str),
                                            None,
                                        )
                                        .await
                                    } else {
                                        mm.post_message_as_bot_with_files_and_override(
                                            &channel_id,
                                            &msg_text,
                                            root,
                                            ts,
                                            &bot_uid,
                                            &bot_token,
                                            &file_ids,
                                            Some(customer_name_str),
                                            None,
                                        )
                                        .await
                                    };
                                    match retry_result {
                                        Ok(post_id) => {
                                            mm.set_root_id(conversation_id, &post_id);
                                            mark_message_synced(state, msg_resp.id, &channel_id)
                                                .await;
                                            posted += 1;
                                        }
                                        Err(e) => {
                                            warn!(
                                                "Retry bot post failed for {}: {}",
                                                conversation_id, e
                                            );
                                            if let Ok(post_id) = mm
                                                .post_message(&channel_id, &msg_text, None, ts)
                                                .await
                                            {
                                                mm.set_root_id(conversation_id, &post_id);
                                                mark_message_synced(
                                                    state,
                                                    msg_resp.id,
                                                    &channel_id,
                                                )
                                                .await;
                                                posted += 1;
                                            }
                                        }
                                    }
                                }
                                Err(e) => {
                                    warn!(
                                        "Bot post failed for {}, falling back: {e}",
                                        conversation_id
                                    );
                                    if let Ok(post_id) =
                                        mm.post_message(&channel_id, &msg_text, msg_root, ts).await
                                    {
                                        if root_id.is_none() {
                                            mm.set_root_id(conversation_id, &post_id);
                                        }
                                        mark_message_synced(state, msg_resp.id, &channel_id).await;
                                        posted += 1;
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            warn!("Bot creation failed for {}, falling back: {e}", cust_id);
                            let fallback_text = if !attachments.is_empty() {
                                let att_md = crate::media::format_attachment_markdown(&attachments);
                                if text.trim().is_empty() {
                                    att_md
                                } else {
                                    format!("{text}\n{att_md}")
                                }
                            } else {
                                text.to_string()
                            };
                            if let Ok(post_id) = mm
                                .post_message(&channel_id, &fallback_text, msg_root, ts)
                                .await
                            {
                                if root_id.is_none() {
                                    mm.set_root_id(conversation_id, &post_id);
                                }
                                mark_message_synced(state, msg_resp.id, &channel_id).await;
                                posted += 1;
                            }
                        }
                    }
                } else {
                    let (msg_text, file_ids) = if !attachments.is_empty() {
                        crate::media::process_attachments_for_post(
                            state,
                            mm,
                            &channel_id,
                            text,
                            &attachments,
                            &msg.id,
                            Some(msg_resp.id),
                            None,
                        )
                        .await
                    } else {
                        (text.to_string(), Vec::new())
                    };

                    let result = if file_ids.is_empty() {
                        mm.post_message(&channel_id, &msg_text, msg_root, ts).await
                    } else {
                        mm.post_message_with_files(&channel_id, &msg_text, msg_root, ts, &file_ids)
                            .await
                    };
                    match result {
                        Ok(post_id) => {
                            if root_id.is_none() {
                                mm.set_root_id(conversation_id, &post_id);
                            }
                            mark_message_synced(state, msg_resp.id, &channel_id).await;
                            posted += 1;
                        }
                        Err(e) if e.to_string().contains("Duplicate post skipped") => {}
                        Err(e) if e.to_string().contains("Skipping empty message") => {}
                        Err(e) => {
                            warn!("Mattermost post failed for {}: {}", conversation_id, e);
                        }
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
