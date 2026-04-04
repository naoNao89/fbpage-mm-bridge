use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::Deserialize;

use crate::AppState;

#[derive(Debug, Deserialize)]
pub struct FbVerifyQuery {
    #[serde(rename = "hub.mode")]
    pub hub_mode: Option<String>,
    #[serde(rename = "hub.verify_token")]
    pub hub_verify_token: Option<String>,
    #[serde(rename = "hub.challenge")]
    pub hub_challenge: Option<String>,
}

pub async fn health_check() -> impl IntoResponse {
    Json(serde_json::json!({ "status": "healthy", "service": "webhook-gateway" }))
}

pub async fn facebook_verify(
    State(state): State<AppState>,
    Query(query): Query<FbVerifyQuery>,
) -> impl IntoResponse {
    tracing::info!("Facebook verification attempt: {:?}", query);

    if query.hub_mode.as_deref() == Some("subscribe")
        && query.hub_verify_token.as_deref() == Some(&state.config.facebook_verify_token)
    {
        let challenge = query.hub_challenge.as_deref().unwrap_or("");
        tracing::info!("Facebook verification successful, returning challenge: {}", challenge);
        return challenge.to_string().into_response();
    }

    tracing::warn!("Facebook verification failed: mode={:?}, token_match={}",
        query.hub_mode,
        query.hub_verify_token.as_deref() == Some(&state.config.facebook_verify_token)
    );

    (StatusCode::FORBIDDEN, "Verification failed").into_response()
}
