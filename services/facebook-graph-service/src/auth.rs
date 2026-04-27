use crate::{config::BypassMode, AppState};
use axum::{http::HeaderMap, Json};
use http::StatusCode;
use serde_json::{json, Value};

pub type AuthResult<T> = Result<T, (StatusCode, Json<Value>)>;

/// Validate `/api/mm-admin/*` bearer token.
///
/// Admin endpoints are callable even when bypass mode is `off`, but only for
/// health/status reads. Mutating handlers should additionally call
/// `ensure_bypass_enabled` before touching Mattermost.
pub fn require_admin_token(headers: &HeaderMap, state: &AppState) -> AuthResult<()> {
    let Some(expected) = state.config.mm_admin_api_token.as_deref() else {
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "MM_ADMIN_API_TOKEN is not configured"})),
        ));
    };

    let provided = headers
        .get(http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "));

    match provided {
        Some(token) if token == expected => Ok(()),
        _ => Err((
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": "invalid or missing bearer token"})),
        )),
    }
}

pub fn ensure_bypass_enabled(state: &AppState) -> AuthResult<()> {
    if state.config.mattermost_bypass_mode != BypassMode::Enabled {
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "Mattermost bypass mode is not enabled"})),
        ));
    }
    Ok(())
}
