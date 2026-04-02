//! Common test utilities for Message Service
//!
//! All test utilities generate dynamic values using UUIDs to avoid
//! hardcoded test data.

use chrono::Utc;
use message_service::models::Message;
use uuid::Uuid;

fn fb_conversation_id() -> String {
    let raw = Uuid::new_v4().to_string().replace("-", "");
    format!("t_{}", &raw[..18])
}

/// Generate a test message with dynamically generated values
pub fn test_message() -> Message {
    let id = Uuid::new_v4();
    let customer_id = Uuid::new_v4();
    let conversation_id = fb_conversation_id();
    let message_text = format!("test_message_{}", Uuid::new_v4());
    let external_id = format!("msg_{}", Uuid::new_v4());

    Message {
        id,
        customer_id,
        conversation_id,
        platform: "facebook".to_string(),
        direction: "incoming".to_string(),
        message_text: Some(message_text),
        external_id: Some(external_id),
        mattermost_channel: None,
        mattermost_synced_at: None,
        mattermost_sync_error: None,
        created_at: Utc::now(),
    }
}

/// Generate a test message for a specific customer
pub fn test_message_for_customer(customer_id: Uuid) -> Message {
    let mut msg = test_message();
    msg.customer_id = customer_id;
    msg
}

/// Generate a test message with dynamically generated Facebook-style data
pub fn test_facebook_message() -> Message {
    let id = Uuid::new_v4();
    let customer_id = Uuid::new_v4();
    let conversation_id = fb_conversation_id();
    let message_text = format!("fb_message_{}", Uuid::new_v4());
    let external_id = format!("m_{}", Uuid::new_v4());

    Message {
        id,
        customer_id,
        conversation_id,
        platform: "facebook".to_string(),
        direction: "incoming".to_string(),
        message_text: Some(message_text),
        external_id: Some(external_id),
        mattermost_channel: None,
        mattermost_synced_at: None,
        mattermost_sync_error: None,
        created_at: Utc::now(),
    }
}
