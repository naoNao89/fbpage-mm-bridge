//! Message Service HTTP client

use anyhow::Context;
use chrono::{DateTime, Utc};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Payload for storing a message
#[derive(Debug, Serialize)]
pub struct MessageServicePayload {
    pub conversation_id: String,
    pub customer_id: Uuid,
    pub platform: String,
    pub direction: String,
    pub message_text: Option<String>,
    pub external_id: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// Message data from Message Service
#[derive(Debug, Clone, Deserialize)]
pub struct MessageServiceResponse {
    pub id: Uuid,
    pub customer_id: Uuid,
    pub platform: String,
    pub direction: String,
    pub message_text: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// Message Service HTTP client
#[derive(Clone)]
pub struct MessageServiceClient {
    base_url: String,
    client: Client,
}

impl MessageServiceClient {
    /// Create a new Message Service client
    pub fn new(base_url: &str) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            client: Client::new(),
        }
    }

    /// Store a message via the Message Service
    pub async fn store_message(
        &self,
        payload: MessageServicePayload,
    ) -> anyhow::Result<MessageServiceResponse> {
        let url = format!("{}/api/messages", self.base_url);

        let response = self
            .client
            .post(&url)
            .json(&payload)
            .send()
            .await
            .context("Failed to send request to Message Service")?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await?;

            // Check for duplicate (already exists)
            if status.as_u16() == 409 || error_text.contains("already exists") {
                return Err(anyhow::anyhow!("message already exists"));
            }

            return Err(anyhow::anyhow!(
                "Message Service returned error {status}: {error_text}"
            ));
        }

        let message: MessageServiceResponse = response
            .json()
            .await
            .context("Failed to parse Message Service response")?;

        Ok(message)
    }

    /// Get message by ID
    pub async fn get_message(&self, id: Uuid) -> anyhow::Result<MessageServiceResponse> {
        let url = format!("{}/api/messages/{}", self.base_url, id);

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .context("Failed to send request to Message Service")?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await?;
            return Err(anyhow::anyhow!(
                "Message Service returned error {status}: {error_text}"
            ));
        }

        let message: MessageServiceResponse = response
            .json()
            .await
            .context("Failed to parse Message Service response")?;

        Ok(message)
    }
}
