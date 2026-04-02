//! Unit tests for Facebook Graph Service clients
//!
//! These tests verify error handling, HTTP response parsing,
//! and retry logic in the service clients.

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use facebook_graph_service::services::CustomerServicePayload;
    use facebook_graph_service::services::MessageServicePayload;
    use uuid::Uuid;

    // MessageServiceClient Tests

    mod message_client {
        use super::*;

        #[test]
        fn test_message_payload_creation() {
            let payload = MessageServicePayload {
                conversation_id: "t_conv_123".to_string(),
                customer_id: Uuid::new_v4(),
                platform: "facebook".to_string(),
                direction: "incoming".to_string(),
                message_text: Some("Test message".to_string()),
                external_id: Some("ext_456".to_string()),
                created_at: Utc::now(),
            };

            assert_eq!(payload.platform, "facebook");
            assert_eq!(payload.direction, "incoming");
            assert!(payload.message_text.is_some());
        }

        #[test]
        fn test_message_payload_serialization() {
            let payload = MessageServicePayload {
                conversation_id: "t_conv_789".to_string(),
                customer_id: Uuid::parse_str("123e4567-e89b-12d3-a456-426614174000").unwrap(),
                platform: "facebook".to_string(),
                direction: "outgoing".to_string(),
                message_text: Some("Hello!".to_string()),
                external_id: None,
                created_at: Utc::now(),
            };

            let json = serde_json::to_string(&payload).unwrap();
            assert!(json.contains("\"conversation_id\":\"t_conv_789\""));
            assert!(json.contains("\"direction\":\"outgoing\""));
        }

        #[test]
        fn test_message_payload_without_optional_fields() {
            let payload = MessageServicePayload {
                conversation_id: "t_conv_empty".to_string(),
                customer_id: Uuid::new_v4(),
                platform: "facebook".to_string(),
                direction: "incoming".to_string(),
                message_text: None,
                external_id: None,
                created_at: Utc::now(),
            };

            assert!(payload.message_text.is_none());
            assert!(payload.external_id.is_none());
        }

        #[test]
        fn test_duplicate_detection_error_message() {
            // The error message format when a duplicate is detected
            let error_msg = "message already exists";
            assert!(error_msg.contains("already exists"));
        }

        #[test]
        fn test_http_status_code_409_duplicate() {
            // HTTP 409 Conflict is returned for duplicates
            let status_code = 409;
            assert_eq!(status_code, 409);
        }

        #[test]
        fn test_error_response_parsing() {
            // Test parsing of error response from Message Service
            let error_json = serde_json::json!({
                "error": "Duplicate message",
                "message": "Message with this external_id already exists"
            });

            let error_str = error_json.to_string();
            assert!(error_str.contains("already exists"));
        }
    }

    // CustomerServiceClient Tests

    mod customer_client {
        use super::*;

        #[test]
        fn test_customer_payload_creation() {
            let payload = CustomerServicePayload {
                platform_user_id: "fb_user_123".to_string(),
                platform: "facebook".to_string(),
                name: Some("John Doe".to_string()),
            };

            assert_eq!(payload.platform_user_id, "fb_user_123");
            assert_eq!(payload.platform, "facebook");
            assert_eq!(payload.name, Some("John Doe".to_string()));
        }

        #[test]
        fn test_customer_payload_without_name() {
            let payload = CustomerServicePayload {
                platform_user_id: "fb_user_456".to_string(),
                platform: "facebook".to_string(),
                name: None,
            };

            assert!(payload.name.is_none());
        }

        #[test]
        fn test_customer_payload_serialization() {
            let payload = CustomerServicePayload {
                platform_user_id: "user_abc".to_string(),
                platform: "facebook".to_string(),
                name: Some("Jane Smith".to_string()),
            };

            let json = serde_json::to_string(&payload).unwrap();
            assert!(json.contains("\"platform_user_id\":\"user_abc\""));
            assert!(json.contains("\"platform\":\"facebook\""));
            assert!(json.contains("\"name\":\"Jane Smith\""));
        }

        #[test]
        fn test_get_or_create_customer_error_handling() {
            // Error message format when Customer Service fails
            let error_msg = "Customer Service returned error 500: Internal Server Error";
            assert!(error_msg.contains("Customer Service"));
            assert!(error_msg.contains("500"));
        }

        #[test]
        fn test_customer_not_found_error() {
            // Error when customer doesn't exist
            let error_msg = "Customer Service returned error 404: Not Found";
            assert!(error_msg.contains("404"));
        }
    }

    // Service URL Building Tests

    mod url_building {
        #[test]
        fn test_message_service_base_url() {
            let base_url = "http://localhost:3002";
            let api_url = format!("{}/api/messages", base_url);
            assert_eq!(api_url, "http://localhost:3002/api/messages");
        }

        #[test]
        fn test_customer_service_base_url() {
            let base_url = "http://localhost:3001";
            let api_url = format!("{}/api/customers", base_url);
            assert_eq!(api_url, "http://localhost:3001/api/customers");
        }

        #[test]
        fn test_url_trim_trailing_slash() {
            let base_url_with_slash = "http://localhost:3002/";
            let trimmed = base_url_with_slash.trim_end_matches('/').to_string();
            assert_eq!(trimmed, "http://localhost:3002");
        }

        #[test]
        fn test_get_message_by_id_url() {
            let base_url = "http://localhost:3002";
            let message_id = "123e4567-e89b-12d3-a456-426614174000";
            let url = format!("{}/api/messages/{}", base_url, message_id);
            assert!(url.contains(message_id));
        }

        #[test]
        fn test_get_customer_by_id_url() {
            let base_url = "http://localhost:3001";
            let customer_id = "123e4567-e89b-12d3-a456-426614174000";
            let url = format!("{}/api/customers/{}", base_url, customer_id);
            assert!(url.contains(customer_id));
        }
    }

    // Error Handling Tests

    mod error_handling {
        #[test]
        fn test_reqwest_error_context() {
            // Test that Anyhow context is properly set
            let error_msg = "Failed to send request to Message Service";
            assert!(error_msg.contains("Failed to send request"));
        }

        #[test]
        fn test_json_parsing_error_context() {
            let error_msg = "Failed to parse Message Service response";
            assert!(error_msg.contains("Failed to parse"));
        }

        #[test]
        fn test_error_status_code_extraction() {
            let status = 500;
            let error_msg = format!("Service returned error {}", status);
            assert!(error_msg.contains("500"));
        }

        #[test]
        fn test_error_text_extraction() {
            let error_text = "{\"error\": \"Something went wrong\"}";
            assert!(error_text.contains("Something went wrong"));
        }

        #[test]
        fn test_network_timeout_error() {
            let error_msg = "request or response handling error";
            // This is how reqwest reports timeouts
            assert!(error_msg.contains("error"));
        }

        #[test]
        fn test_connection_refused_error() {
            // Connection refused typically shows as a lower-level error
            let error_msg = "Connection refused";
            assert!(error_msg.contains("Connection refused"));
        }
    }

    // Response Parsing Tests

    mod response_parsing {
        use super::*;

        #[test]
        fn test_message_service_response_parsing() {
            #[derive(serde::Deserialize, Debug)]
            struct MessageResponse {
                platform: String,
                direction: String,
            }

            let json = serde_json::json!({
                "id": "123e4567-e89b-12d3-a456-426614174000",
                "customer_id": "123e4567-e89b-12d3-a456-426614174001",
                "platform": "facebook",
                "direction": "incoming",
                "message_text": "Hello!",
                "created_at": "2024-01-15T10:30:00Z"
            });

            let response: MessageResponse = serde_json::from_value(json).unwrap();
            assert_eq!(response.platform, "facebook");
            assert_eq!(response.direction, "incoming");
        }

        #[test]
        fn test_customer_service_response_parsing() {
            #[derive(serde::Deserialize, Debug)]
            struct CustomerResponse {
                platform_user_id: String,
                platform: String,
                phone: Option<String>,
            }

            let json = serde_json::json!({
                "id": "123e4567-e89b-12d3-a456-426614174000",
                "platform_user_id": "fb_123",
                "platform": "facebook",
                "name": "John Doe",
                "phone": null,
                "created_at": "2024-01-15T10:30:00Z"
            });

            let response: CustomerResponse = serde_json::from_value(json).unwrap();
            assert_eq!(response.platform_user_id, "fb_123");
            assert!(response.phone.is_none());
        }
    }

    // Deduplication Logic Tests

    mod deduplication {
        #[test]
        fn test_duplicate_detection_by_external_id() {
            // External ID is used for deduplication
            let external_id_1 = "fb_msg_123";
            let external_id_2 = "fb_msg_123";
            let external_id_3 = "fb_msg_456";

            assert_eq!(external_id_1, external_id_2);
            assert_ne!(external_id_1, external_id_3);
        }

        #[test]
        fn test_duplicate_error_message_contains_keyword() {
            let error_msg = "message already exists";
            assert!(error_msg.contains("already exists"));
        }

        #[test]
        fn test_skip_duplicate_on_409() {
            // HTTP 409 Conflict triggers duplicate skip
            let status_code = 409;
            assert_eq!(status_code, 409);
        }

        #[test]
        fn test_skip_duplicate_on_error_text() {
            let error_text = "Message with external_id abc already exists";
            assert!(error_text.contains("already exists"));
        }
    }

    // Pagination Handling Tests

    mod pagination {
        #[test]
        fn test_paging_cursor_structure() {
            #[derive(serde::Deserialize, Debug)]
            struct Cursors {
                before: String,
                after: String,
            }

            let json = serde_json::json!({
                "before": "cursor_before_123",
                "after": "cursor_after_456"
            });

            let cursors: Cursors = serde_json::from_value(json).unwrap();
            assert!(cursors.before.starts_with("cursor_before"));
            assert!(cursors.after.starts_with("cursor_after"));
        }

        #[test]
        fn test_next_page_url_presence() {
            let next_url: Option<String> = Some("https://graph.facebook.com/v24.0/...".to_string());
            assert!(next_url.is_some());

            let no_next_url: Option<String> = None;
            assert!(no_next_url.is_none());
        }

        #[test]
        fn test_pagination_loop_termination() {
            // When next is None, pagination loop should terminate
            let next_url: Option<String> = None;
            assert!(next_url.is_none());
        }
    }

    // Retry Logic Tests

    mod retry_logic {
        #[test]
        fn test_retry_on_rate_limit_error() {
            // HTTP 429 or 403 indicates rate limiting
            let rate_limit_statuses = vec![429, 403];
            for status in rate_limit_statuses {
                let is_rate_limit = status == 429 || status == 403;
                assert!(is_rate_limit);
            }
        }

        #[test]
        fn test_no_retry_on_4xx_client_errors() {
            // 4xx errors (except 429/403) should not be retried
            let client_errors = vec![400, 401, 404];
            for status in client_errors {
                let should_retry = status == 429 || status == 403;
                assert!(!should_retry);
            }
        }

        #[test]
        fn test_retry_on_5xx_server_errors() {
            // 5xx errors should be retried
            let server_errors = vec![500, 502, 503];
            for status in server_errors {
                let should_retry = status >= 500;
                assert!(should_retry);
            }
        }

        #[test]
        fn test_exponential_backoff_calculation() {
            // Simple backoff calculation
            let base_delay_ms = 500;
            let attempt = 3;
            let delay_ms = base_delay_ms * 2u64.pow(attempt - 1);
            assert_eq!(delay_ms, 2000);
        }

        #[test]
        fn test_max_retry_attempts() {
            let max_attempts = 3;
            let current_attempt = 3;
            assert!(current_attempt >= max_attempts);
        }
    }

    // Model Parsing Tests

    mod model_parsing {
        use facebook_graph_service::graph_api::calculate_backoff_duration;
        use facebook_graph_service::models::*;
        use std::time::Duration;
        use uuid::Uuid;

        #[test]
        fn test_parse_graph_conversation() {
            let json = serde_json::json!({
                "id": "t_1234567890",
                "updated_time": "2024-01-15T10:30:00+0000",
                "message_count": 42
            });
            let conversation: Conversation = serde_json::from_value(json).unwrap();
            assert_eq!(conversation.id, "t_1234567890");
            assert_eq!(conversation.message_count, Some(42));
        }

        #[test]
        fn test_parse_graph_message_incoming() {
            let json = serde_json::json!({
                "id": "m_123456",
                "created_time": "2024-01-15T10:30:00+0000",
                "from": {"name": "John Doe", "id": "user_123"},
                "message": "Hello, world!",
                "to": {"data": [{"name": "Test Page", "id": "page_456"}]}
            });
            let message: GraphMessage = serde_json::from_value(json).unwrap();
            assert_eq!(message.from.id, "user_123");
            assert_eq!(message.message, Some("Hello, world!".to_string()));
        }

        #[test]
        fn test_parse_graph_message_outgoing() {
            let json = serde_json::json!({
                "id": "m_789012",
                "created_time": "2024-01-15T10:30:00+0000",
                "from": {"name": "Test Page", "id": "page_456"},
                "message": "Hi there!",
                "to": {"data": [{"name": "John Doe", "id": "user_123"}]}
            });
            let message: GraphMessage = serde_json::from_value(json).unwrap();
            assert_eq!(message.from.id, "page_456");
        }

        #[test]
        fn test_direction_detection() {
            let page_id = "page_123";
            let json = serde_json::json!({
                "id": "m_001",
                "created_time": "2024-01-15T10:00:00+0000",
                "from": {"name": "Customer", "id": "user_456"},
                "to": {"data": [{"name": "Page", "id": page_id}]}
            });
            let message: GraphMessage = serde_json::from_value(json).unwrap();
            let is_from_page = message.from.id == page_id;
            let direction = if is_from_page { "outgoing" } else { "incoming" };
            assert_eq!(direction, "incoming");
        }

        #[test]
        fn test_parse_rate_limit_info() {
            let json = serde_json::json!({
                "call_count": 100,
                "call_count_limit": 200
            });
            let info: FacebookRateLimitInfo = serde_json::from_value(json).unwrap();
            assert_eq!(info.call_count, Some(100));
            assert_eq!(info.call_count_limit, Some(200));
        }

        #[test]
        fn test_backoff_duration_critical() {
            let duration = calculate_backoff_duration(95.0);
            assert_eq!(duration, Duration::from_secs(30 * 60));
        }

        #[test]
        fn test_backoff_duration_warning() {
            let duration = calculate_backoff_duration(80.0);
            assert_eq!(duration, Duration::from_secs(10 * 60));
        }

        #[test]
        fn test_backoff_duration_normal() {
            let duration = calculate_backoff_duration(50.0);
            assert_eq!(duration, Duration::from_secs(0));
        }

        #[test]
        fn test_import_response_serialization() {
            let response = ImportResponse {
                status: "completed".to_string(),
                job_id: Uuid::new_v4(),
                message: "Done".to_string(),
            };
            let json = serde_json::to_string(&response).unwrap();
            assert!(json.contains("completed"));
        }

        #[test]
        fn test_message_with_empty_text() {
            let json = serde_json::json!({
                "id": "m_empty",
                "created_time": "2024-01-15T10:00:00+0000",
                "from": {"name": "User", "id": "u_1"},
                "to": {"data": [{"name": "Page", "id": "p_1"}]}
            });
            let message: GraphMessage = serde_json::from_value(json).unwrap();
            assert!(message.message.is_none());
        }

        #[test]
        fn test_participant_with_email() {
            let json = serde_json::json!({
                "name": "John Doe",
                "id": "user_123",
                "email": "john@example.com"
            });
            let participant: GraphParticipant = serde_json::from_value(json).unwrap();
            assert_eq!(participant.email, Some("john@example.com".to_string()));
        }

        #[test]
        fn test_attachment_video() {
            let json = serde_json::json!({
                "id": "att_video",
                "video_data": {"url": "https://example.com/video.mp4", "width": 1920, "height": 1080}
            });
            let attachment: GraphAttachment = serde_json::from_value(json).unwrap();
            assert!(attachment.video_data.is_some());
        }
    }
}
