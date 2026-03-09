use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

/// Customer entity representing a user from a platform (Facebook, Zalo, etc.)
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Customer {
    pub id: Uuid,
    pub platform_user_id: String,
    pub platform: String, // "facebook" or "zalo"
    pub name: Option<String>,
    pub phone: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// Request payload for creating or getting a customer
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateCustomerRequest {
    pub platform_user_id: String,
    pub platform: String,
    pub name: Option<String>,
    pub phone: Option<String>,
}

/// Request payload for updating a customer
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateCustomerRequest {
    pub name: Option<String>,
    pub phone: Option<String>,
}

/// Response for customer API endpoints
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomerResponse {
    pub id: Uuid,
    pub platform_user_id: String,
    pub platform: String,
    pub name: Option<String>,
    pub phone: Option<String>,
    pub created_at: DateTime<Utc>,
}

impl From<Customer> for CustomerResponse {
    fn from(customer: Customer) -> Self {
        Self {
            id: customer.id,
            platform_user_id: customer.platform_user_id,
            platform: customer.platform,
            name: customer.name,
            phone: customer.phone,
            created_at: customer.created_at,
        }
    }
}

/// Query parameters for listing customers
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListCustomersQuery {
    pub platform: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

impl Default for ListCustomersQuery {
    fn default() -> Self {
        Self {
            platform: None,
            limit: Some(50),
            offset: Some(0),
        }
    }
}