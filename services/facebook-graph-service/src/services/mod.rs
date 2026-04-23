//! Service clients for communicating with other microservices

mod customer_client;
mod mattermost_client;
mod message_client;

pub use customer_client::{CustomerServiceClient, CustomerServicePayload, CustomerServiceResponse};
pub use mattermost_client::{ChannelInfo, MattermostClient, MattermostPost};
pub use message_client::{AttachmentPayload, MarkSyncedPayload, MessageServiceClient, MessageServicePayload};
