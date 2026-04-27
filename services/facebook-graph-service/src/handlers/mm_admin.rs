use crate::auth::{ensure_bypass_enabled, require_admin_token};
use crate::AppState;
use axum::{
    extract::{Path, State},
    http::HeaderMap,
    Json,
};
use http::StatusCode;
use serde::Deserialize;
use serde_json::{json, Value};

pub type HandlerResult<T> = Result<Json<T>, (StatusCode, Json<Value>)>;

#[derive(Debug, Deserialize)]
pub struct SendDmRequest {
    pub from_user_id: String,
    pub to_user_id: String,
    pub message: String,
    pub idempotency_key: Option<String>,
}

pub async fn admin_health(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> HandlerResult<Value> {
    require_admin_token(&headers, &state)?;
    let schema_version = state
        .mattermost_ops
        .schema_version()
        .await
        .unwrap_or_else(|e| {
            tracing::warn!("Failed to read Mattermost schema version: {e}");
            None
        });

    Ok(Json(json!({
        "mode": state.config.mattermost_bypass_mode.as_str(),
        "mm_db": state.mattermost_ops.has_db(),
        "schema_version": schema_version,
    })))
}

pub async fn delete_channel_posts(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(channel_id): Path<String>,
) -> HandlerResult<Value> {
    require_admin_token(&headers, &state)?;
    ensure_bypass_enabled(&state)?;
    let result = state
        .mattermost_ops
        .delete_all_posts_in_channel(&channel_id)
        .await
        .map_err(internal_error)?;

    Ok(Json(json!({
        "deleted": result.result.deleted,
        "path": result.path,
        "duration_ms": result.duration_ms,
    })))
}

pub async fn send_dm(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<SendDmRequest>,
) -> HandlerResult<Value> {
    require_admin_token(&headers, &state)?;
    ensure_bypass_enabled(&state)?;
    let idempotency_key = payload
        .idempotency_key
        .as_deref()
        .or_else(|| header_idempotency_key(&headers));
    let result = state
        .mattermost_ops
        .send_dm(
            &payload.from_user_id,
            &payload.to_user_id,
            &payload.message,
            idempotency_key,
        )
        .await
        .map_err(internal_error)?;

    Ok(Json(json!({
        "post_id": result.result.post_id,
        "channel_id": result.result.channel_id,
        "path": result.path,
        "duration_ms": result.duration_ms,
    })))
}

pub async fn archive_channel(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(channel_id): Path<String>,
) -> HandlerResult<Value> {
    require_admin_token(&headers, &state)?;
    ensure_bypass_enabled(&state)?;
    let result = state
        .mattermost_ops
        .archive_channel(&channel_id)
        .await
        .map_err(internal_error)?;
    Ok(Json(
        json!({"ok": true, "path": result.path, "duration_ms": result.duration_ms}),
    ))
}

pub async fn unarchive_channel(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(channel_id): Path<String>,
) -> HandlerResult<Value> {
    require_admin_token(&headers, &state)?;
    ensure_bypass_enabled(&state)?;
    let result = state
        .mattermost_ops
        .unarchive_channel(&channel_id)
        .await
        .map_err(internal_error)?;
    Ok(Json(
        json!({"ok": true, "path": result.path, "duration_ms": result.duration_ms}),
    ))
}

fn header_idempotency_key(headers: &HeaderMap) -> Option<&str> {
    headers
        .get("Idempotency-Key")
        .and_then(|value| value.to_str().ok())
}

fn internal_error(error: anyhow::Error) -> (StatusCode, Json<Value>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({"error": error.to_string()})),
    )
}
