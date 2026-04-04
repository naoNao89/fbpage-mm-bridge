pub mod config;
pub mod handlers;

use axum::{routing::get, Router};

use crate::config::Config;
use crate::handlers::{facebook_verify, health_check};

#[derive(Clone)]
pub struct AppState {
    pub config: Config,
}

pub fn create_app(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health_check))
        .route("/webhook/facebook", get(facebook_verify))
        .with_state(state)
}
