//! Service clients for communicating with other microservices

mod customer_client;
mod mattermost_client;
mod mattermost_db;
mod message_client;

pub use customer_client::{CustomerServiceClient, CustomerServicePayload, CustomerServiceResponse};
pub use mattermost_client::{ChannelInfo, MattermostClient, MattermostPost};
pub use mattermost_db::{ChannelDbInfo, MattermostDbClient};
pub use message_client::{
    AttachmentPayload, MarkSyncedPayload, MessageServiceClient, MessageServicePayload,
};
