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

#[derive(Debug, Serialize)]
pub struct MarkSyncedPayload {
    pub mattermost_channel: String,
}

/// Message Service HTTP client
#[derive(Clone)]
pub struct MessageServiceClient {
    base_url: String,
    client: Client,
}

#[derive(Debug, Clone, Serialize)]
pub struct AttachmentPayload {
    pub message_id: Uuid,
    pub attachment_type: String,
    pub external_id: Option<String>,
    pub name: Option<String>,
    pub mime_type: Option<String>,
    pub size_bytes: Option<i64>,
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub cdn_url: Option<String>,
    pub cdn_url_expires_at: Option<chrono::DateTime<chrono::Utc>>,
    pub minio_key: Option<String>,
    pub minio_bucket: Option<String>,
    pub minio_etag: Option<String>,
    pub mm_file_id: Option<String>,
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
        let url = format!("{}/api/messages/{id}", self.base_url);

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

    pub async fn store_attachment(&self, payload: AttachmentPayload) -> anyhow::Result<()> {
        let url = format!("{}/api/attachments", self.base_url);

        let response = self
            .client
            .post(&url)
            .json(&payload)
            .send()
            .await
            .context("Failed to send attachment to Message Service")?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await?;
            return Err(anyhow::anyhow!(
                "Message Service attachment error {status}: {error_text}"
            ));
        }

        Ok(())
    }

    pub async fn update_attachment_mm_file_id(
        &self,
        attachment_id: Uuid,
        mm_file_id: &str,
    ) -> anyhow::Result<()> {
        let url = format!(
            "{}/api/attachments/{attachment_id}/mm-file-id",
            self.base_url
        );

        let payload = serde_json::json!({ "mm_file_id": mm_file_id });

        let response = self
            .client
            .put(&url)
            .json(&payload)
            .send()
            .await
            .context("Failed to update attachment mm_file_id")?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await?;
            return Err(anyhow::anyhow!(
                "Message Service attachment update error {status}: {error_text}"
            ));
        }

        Ok(())
    }

    pub async fn mark_synced(
        &self,
        message_id: Uuid,
        mattermost_channel: &str,
    ) -> anyhow::Result<()> {
        let url = format!("{}/api/messages/{message_id}/synced", self.base_url);

        let payload = MarkSyncedPayload {
            mattermost_channel: mattermost_channel.to_string(),
        };

        let response = self
            .client
            .put(&url)
            .json(&payload)
            .send()
            .await
            .context("Failed to send mark_synced request to Message Service")?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await?;
            anyhow::bail!("Message Service mark_synced returned error {status}: {error_text}");
        }

        Ok(())
    }
}
