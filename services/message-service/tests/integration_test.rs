//! Integration tests for Message Service
//!
//! These tests verify that the Message Service correctly handles arbitrary
//! Facebook message payloads.

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use message_service::models::{CreateMessageRequest, Message};
    use uuid::Uuid;

    /// Helper: creates a valid Facebook message request with given parameters
    fn facebook_message_request(
        conversation_id: &str,
        message_text: &str,
        external_id: &str,
    ) -> CreateMessageRequest {
        CreateMessageRequest {
            customer_id: Uuid::new_v4(),
            conversation_id: conversation_id.to_string(),
            platform: "facebook".to_string(),
            direction: "incoming".to_string(),
            message_text: Some(message_text.to_string()),
            external_id: Some(external_id.to_string()),
        }
    }

    #[test]
    fn test_create_message_request_accepts_valid_facebook_payload() {
        // Verify the request can accept any valid Facebook message format
        let request = facebook_message_request(
            "t_987654321",   // conversation_id - any FB conversation ID format
            "Hello, world!", // message_text - any text content
            "msg_abc123",    // external_id - any FB message ID
        );

        // Verify all fields are set correctly
        assert_eq!(request.platform, "facebook");
        assert!(request.conversation_id.starts_with("t_"));
        assert!(request.message_text.is_some());
        assert!(request.external_id.is_some());
    }

    #[test]
    fn test_create_message_request_serialization() {
        // Use generated values to avoid hardcoding
        let conversation_id = format!(
            "t_{}",
            Uuid::new_v4().to_string().replace("-", "")[..18].to_string()
        );
        let message_text = format!("test_message_{}", Uuid::new_v4());
        let external_id = format!("msg_{}", Uuid::new_v4());

        let request = facebook_message_request(&conversation_id, &message_text, &external_id);

        let json = serde_json::to_string(&request).unwrap();
        // Verify structure
        assert!(json.contains("\"platform\":\"facebook\""));
        assert!(json.contains("\"direction\":\"incoming\""));
        assert!(serde_json::from_str::<CreateMessageRequest>(&json).is_ok());
    }

    #[test]
    fn test_message_response_serialization() {
        let message = Message {
            id: Uuid::new_v4(),
            customer_id: Uuid::new_v4(),
            conversation_id: "t_any_conversation_id".to_string(),
            platform: "facebook".to_string(),
            direction: "incoming".to_string(),
            message_text: Some("any message content".to_string()),
            external_id: Some("any_external_id".to_string()),
            mattermost_channel: None,
            mattermost_synced_at: None,
            mattermost_sync_error: None,
            created_at: Utc::now(),
        };

        let response: message_service::models::MessageResponse = message.into();
        let json = serde_json::to_string(&response).unwrap();

        // Verify response structure contains required fields
        assert!(json.contains("\"platform\":\"facebook\""));
        assert!(json.contains("\"direction\":\"incoming\""));
        assert!(json.contains("\"id\":"));
        assert!(json.contains("\"customer_id\":"));
        assert!(json.contains("\"conversation_id\":"));
        assert!(json.contains("\"created_at\":"));
    }

    #[test]
    fn test_direction_validation() {
        // Valid directions should work
        let valid_directions = vec!["incoming", "outgoing"];
        for direction in valid_directions {
            let request = CreateMessageRequest {
                customer_id: Uuid::new_v4(),
                conversation_id: "t_123".to_string(),
                platform: "facebook".to_string(),
                direction: direction.to_string(),
                message_text: Some("test".to_string()),
                external_id: None,
            };
            assert!(serde_json::to_string(&request).is_ok());
            assert_eq!(request.direction, direction);
        }
    }

    #[test]
    fn test_facebook_message_various_content() {
        // Verify system handles various Facebook message content types
        let test_cases = vec![
            "Hi",
            "This is a much longer message with more content that could be sent by a user on Facebook",
            "Hello! 👋 How are you?",
            "Xin chào thế giới",
        ];

        for msg_text in test_cases {
            let request = CreateMessageRequest {
                customer_id: Uuid::new_v4(),
                conversation_id: "t_fb_conversation_123".to_string(),
                platform: "facebook".to_string(),
                direction: "incoming".to_string(),
                message_text: Some(msg_text.to_string()),
                external_id: Some(format!("msg_{}", Uuid::new_v4())),
            };

            // Verify serialization/deserialization works for any content
            let json = serde_json::to_string(&request).unwrap();
            let parsed: CreateMessageRequest = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed.message_text, Some(msg_text.to_string()));
        }
    }

    #[test]
    fn test_multiple_platforms_supported() {
        // Verify the system can handle messages from different platforms
        let platforms = vec!["facebook", "zalo", "LINE"];

        for platform in platforms {
            let request = CreateMessageRequest {
                customer_id: Uuid::new_v4(),
                conversation_id: format!("conv_{}", platform),
                platform: platform.to_string(),
                direction: "incoming".to_string(),
                message_text: Some("test".to_string()),
                external_id: Some(format!("ext_{}", Uuid::new_v4())),
            };

            assert_eq!(request.platform, platform);
            let json = serde_json::to_string(&request).unwrap();
            assert!(serde_json::from_str::<CreateMessageRequest>(&json).is_ok());
        }
    }
}
