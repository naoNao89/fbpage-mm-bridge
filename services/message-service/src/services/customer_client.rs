//! Customer Service Client
//!
//! HTTP client for communicating with the Customer Service to validate
//! customer existence before creating messages.

use reqwest::Client;
use serde::Deserialize;
use thiserror::Error;
use uuid::Uuid;

#[derive(Error, Debug)]
pub enum CustomerClientError {
    #[error("Customer not found: {0}")]
    NotFound(Uuid),
    #[error("Customer service unavailable: {0}")]
    Unavailable(String),
    #[error("Request failed: {0}")]
    RequestFailed(#[from] reqwest::Error),
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct CustomerResponse {
    id: Uuid,
    platform_user_id: String,
    platform: String,
    name: Option<String>,
}

/// Client for communicating with the Customer Service
#[derive(Clone)]
pub struct CustomerServiceClient {
    base_url: String,
    client: Client,
}

impl CustomerServiceClient {
    /// Create a new CustomerServiceClient
    pub fn new(base_url: &str) -> Self {
        Self {
            base_url: base_url.to_string(),
            client: Client::new(),
        }
    }

    /// Check if a customer exists by ID
    pub async fn customer_exists(&self, customer_id: Uuid) -> Result<bool, CustomerClientError> {
        let url = format!("{}/api/customers/{customer_id}", self.base_url);

        let response = self.client.get(&url).send().await?;

        match response.status() {
            reqwest::StatusCode::OK => Ok(true),
            reqwest::StatusCode::NOT_FOUND => Ok(false),
            status => Err(CustomerClientError::Unavailable(format!(
                "Customer service returned status: {status}"
            ))),
        }
    }

    /// Get customer by ID
    pub async fn get_customer(
        &self,
        customer_id: Uuid,
    ) -> Result<Option<CustomerResponse>, CustomerClientError> {
        let url = format!("{}/api/customers/{customer_id}", self.base_url);

        let response = self.client.get(&url).send().await?;

        match response.status() {
            reqwest::StatusCode::OK => {
                let customer = response.json::<CustomerResponse>().await?;
                Ok(Some(customer))
            }
            reqwest::StatusCode::NOT_FOUND => Ok(None),
            status => Err(CustomerClientError::Unavailable(format!(
                "Customer service returned status: {status}"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_creation() {
        let client = CustomerServiceClient::new("http://localhost:3001");
        assert_eq!(client.base_url, "http://localhost:3001");
    }
}
