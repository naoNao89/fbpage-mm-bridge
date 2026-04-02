//! Customer Service HTTP client

use anyhow::Context;
use chrono::{DateTime, Utc};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Customer data from Customer Service
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomerServiceResponse {
    pub id: Uuid,
    pub platform_user_id: String,
    pub platform: String,
    pub name: Option<String>,
    pub phone: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// Payload for creating/getting a customer
#[derive(Debug, Serialize)]
pub struct CustomerServicePayload {
    pub platform_user_id: String,
    pub platform: String,
    pub name: Option<String>,
}

/// Customer Service HTTP client
#[derive(Clone)]
pub struct CustomerServiceClient {
    base_url: String,
    client: Client,
}

impl CustomerServiceClient {
    /// Create a new Customer Service client
    pub fn new(base_url: &str) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            client: Client::new(),
        }
    }

    /// Get or create a customer by platform ID
    pub async fn get_or_create_customer(
        &self,
        platform_user_id: &str,
        platform: &str,
        name: Option<&str>,
    ) -> anyhow::Result<CustomerServiceResponse> {
        let url = format!("{}/api/customers", self.base_url);

        let payload = CustomerServicePayload {
            platform_user_id: platform_user_id.to_string(),
            platform: platform.to_string(),
            name: name.map(String::from),
        };

        let response = self
            .client
            .post(&url)
            .json(&payload)
            .send()
            .await
            .context("Failed to send request to Customer Service")?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await?;
            return Err(anyhow::anyhow!(
                "Customer Service returned error {status}: {error_text}"
            ));
        }

        let customer: CustomerServiceResponse = response
            .json()
            .await
            .context("Failed to parse Customer Service response")?;

        Ok(customer)
    }

    /// Get customer by ID
    pub async fn get_customer(&self, id: Uuid) -> anyhow::Result<CustomerServiceResponse> {
        let url = format!("{}/api/customers/{id}", self.base_url);

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .context("Failed to send request to Customer Service")?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await?;
            return Err(anyhow::anyhow!(
                "Customer Service returned error {status}: {error_text}"
            ));
        }

        let customer: CustomerServiceResponse = response
            .json()
            .await
            .context("Failed to parse Customer Service response")?;

        Ok(customer)
    }
}
