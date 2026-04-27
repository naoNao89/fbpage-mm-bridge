//! Service clients for communicating with other microservices

pub use crate::config::BypassMode;

mod customer_client;
mod mattermost_client;
mod mattermost_db;
mod mattermost_ops;
mod message_client;

pub use customer_client::{CustomerServiceClient, CustomerServicePayload, CustomerServiceResponse};
pub use mattermost_client::{ChannelInfo, MattermostClient, MattermostPost};
pub use mattermost_db::{ChannelDbInfo, MattermostDbClient};
pub use mattermost_ops::{DeletePostsResult, MattermostOps, OperationResult, SendDmResult};
pub use message_client::{
    AttachmentPayload, MarkSyncedPayload, MessageServiceClient, MessageServicePayload,
};
