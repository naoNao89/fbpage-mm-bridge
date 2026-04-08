use crate::AppState;
use std::time::Duration;
use tracing::{error, info, warn};

pub async fn run_poller(state: AppState, interval_secs: u64) {
    let mut last_poll_ts = chrono::Utc::now().timestamp_millis() - (interval_secs as i64 * 1000);

    loop {
        tokio::time::sleep(Duration::from_secs(interval_secs)).await;

        info!("Poller: checking for conversations updated after {}", last_poll_ts);

        match poll_new_conversations(&state, last_poll_ts).await {
            Ok(count) => {
                info!("Poller: processed {} conversations", count);
                last_poll_ts = chrono::Utc::now().timestamp_millis();
            }
            Err(e) => {
                error!("Poller error: {}", e);
                tokio::time::sleep(Duration::from_secs(10)).await;
            }
        }
    }
}

async fn poll_new_conversations(state: &AppState, since: i64) -> anyhow::Result<usize> {
    let conversations = crate::graph_api::get_conversations(&state.pool, &state.config).await?;

    let mut processed = 0;
    let mut newest_ts = since;
    for conv in &conversations {
        let updated_at = conv.updated_time.timestamp_millis();
        if updated_at > newest_ts {
            newest_ts = updated_at;
        }
        if updated_at <= since {
            continue;
        }

        let result = crate::handlers::process_conversation(
            state,
            &conv.id,
            &state.config.facebook_page_id,
            &state.config.facebook_page_access_token,
        )
        .await;

        match result {
            Ok(r) => {
                info!(
                    "Poller: processed conversation {} ({} messages)",
                    conv.id, r.messages_stored
                );
                processed += 1;
            }
            Err(e) => {
                warn!("Poller: failed to process conversation {}: {}", conv.id, e);
            }
        }
    }

    Ok(processed)
}