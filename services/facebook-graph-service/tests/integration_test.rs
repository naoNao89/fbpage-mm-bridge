//! Integration tests for Facebook Graph Service
//!
//! Tests that require real Facebook API, external services, or full flow.
//! Run with: cargo test -- --ignored

#[cfg(test)]
mod tests {
    mod real_api_tests {
        use chrono::Utc;
        use facebook_graph_service::config::Config;
        use facebook_graph_service::graph_api;
        use facebook_graph_service::services::{CustomerServiceClient, MessageServiceClient, MessageServicePayload};
        use sqlx::postgres::PgPoolOptions;
        use std::time::Duration;
        use uuid::Uuid;

        fn init_env() -> Config {
            dotenvy::dotenv().ok();
            Config::from_env().expect("Failed to load configuration")
        }

        async fn create_pool(database_url: &str) -> sqlx::PgPool {
            PgPoolOptions::new()
                .max_connections(5)
                .acquire_timeout(Duration::from_secs(10))
                .connect(database_url)
                .await
                .expect("Failed to create pool")
        }

        #[tokio::test]
        #[ignore]
        async fn test_real_token_validation() {
            let config = init_env();
            let token_type = graph_api::debug_token(
                &config.facebook_page_access_token,
                &config.facebook_app_id,
                &config.facebook_app_secret,
            )
            .await
            .expect("Failed to validate token");
            assert!(token_type == "PAGE" || token_type == "USER");
        }

        #[tokio::test]
        #[ignore]
        async fn test_real_fetch_conversations() {
            let config = init_env();
            let pool = create_pool(&config.database_url).await;
            let _ = sqlx::migrate!("./migrations").run(&pool).await;

            let conversations = graph_api::get_conversations(&pool, &config)
                .await
                .expect("Failed to fetch conversations");

            println!("Total conversations: {}", conversations.len());
        }

        #[tokio::test]
        #[ignore]
        async fn test_real_fetch_messages() {
            let config = init_env();
            let pool = create_pool(&config.database_url).await;

            let conversations = graph_api::get_conversations(&pool, &config)
                .await
                .expect("Failed to fetch conversations");

            if conversations.is_empty() {
                return;
            }

            let messages = graph_api::get_conversation_messages(
                &pool,
                &conversations[0].id,
                &config.facebook_page_access_token,
            )
            .await
            .expect("Failed to fetch messages");

            println!("Total messages: {}", messages.len());
        }

        #[tokio::test]
        #[ignore]
        async fn test_real_customer_service() {
            let config = init_env();
            let client = CustomerServiceClient::new(&config.customer_service_url);

            let result = client
                .get_or_create_customer("test_user_123", "facebook", Some("Test User"))
                .await;

            assert!(result.is_ok());
        }

        #[tokio::test]
        #[ignore]
        async fn test_real_message_service() {
            let config = init_env();
            let customer_client = CustomerServiceClient::new(&config.customer_service_url);
            let message_client = MessageServiceClient::new(&config.message_service_url);

            let customer = customer_client
                .get_or_create_customer("msg_test_user", "facebook", Some("Msg Test"))
                .await
                .expect("Failed to create customer");

            let payload = MessageServicePayload {
                conversation_id: format!("t_test_{}", Uuid::new_v4()),
                customer_id: customer.id,
                platform: "facebook".to_string(),
                direction: "incoming".to_string(),
                message_text: Some("Test message".to_string()),
                external_id: Some(format!("test_{}", Uuid::new_v4())),
                created_at: Utc::now(),
            };

            let result = message_client.store_message(payload).await;
            assert!(result.is_ok());
        }

        #[tokio::test]
        #[ignore]
        async fn test_real_full_import_flow() {
            let config = init_env();
            let pool = create_pool(&config.database_url).await;
            let _ = sqlx::migrate!("./migrations").run(&pool).await;

            let conversations = graph_api::get_conversations(&pool, &config)
                .await
                .expect("Failed to fetch conversations");

            if conversations.is_empty() {
                println!("No conversations");
                return;
            }

            let messages = graph_api::get_conversation_messages(
                &pool,
                &conversations[0].id,
                &config.facebook_page_access_token,
            )
            .await
            .expect("Failed to fetch messages");

            let customer_client = CustomerServiceClient::new(&config.customer_service_url);
            let message_client = MessageServiceClient::new(&config.message_service_url);

            for msg in messages.iter().take(5) {
                let (direction, customer_id, customer_name) = if msg.from.id == config.facebook_page_id {
                    let recipient = msg.to.data.first()
                        .map(|p| (p.id.clone(), p.name.clone()))
                        .unwrap_or_else(|| ("unknown".to_string(), "Unknown".to_string()));
                    ("outgoing".to_string(), recipient.0, Some(recipient.1))
                } else {
                    ("incoming".to_string(), msg.from.id.clone(), Some(msg.from.name.clone()))
                };

                let customer = match customer_client
                    .get_or_create_customer(&customer_id, "facebook", customer_name.as_deref())
                    .await
                {
                    Ok(c) => c,
                    Err(_) => continue,
                };

                let payload = MessageServicePayload {
                    conversation_id: conversations[0].id.clone(),
                    customer_id: customer.id,
                    platform: "facebook".to_string(),
                    direction,
                    message_text: msg.message.clone(),
                    external_id: Some(msg.id.clone()),
                    created_at: msg.created_time,
                };

                let _ = message_client.store_message(payload).await;
            }

            println!("Import completed");
        }

        #[tokio::test]
        #[ignore]
        async fn test_real_rate_limit_tracking() {
            let config = init_env();
            let pool = create_pool(&config.database_url).await;
            let _ = sqlx::migrate!("./migrations").run(&pool).await;

            let _ = graph_api::get_conversations(&pool, &config).await;

            let status = graph_api::check_rate_limit_status(&pool, "conversations")
                .await
                .expect("Failed to check rate limit");

            println!("Rate limit: {:.1}%", status.usage_percent);
        }
    }
}
