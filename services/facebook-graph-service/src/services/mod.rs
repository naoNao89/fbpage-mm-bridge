//! Service clients for communicating with other microservices

mod customer_client;
mod message_client;
mod mattermost_client;

pub use customer_client::{CustomerServiceClient, CustomerServicePayload, CustomerServiceResponse};
pub use message_client::{MessageServiceClient, MessageServicePayload};
pub use mattermost_client::MattermostClient;
