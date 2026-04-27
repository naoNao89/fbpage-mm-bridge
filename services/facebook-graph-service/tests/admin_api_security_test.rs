use axum::{
    body::Body,
    http::{Method, Request, StatusCode},
};
use facebook_graph_service::{
    config::{BypassMode, Config},
    create_app,
    services::{CustomerServiceClient, MattermostClient, MattermostOps, MessageServiceClient},
    AppState,
};
use serde_json::Value;
use sqlx::postgres::PgPoolOptions;
use std::{collections::HashMap, sync::Arc};
use tokio::sync::RwLock;
use tower::ServiceExt;

fn test_config(mode: BypassMode, token: Option<&str>) -> Config {
    Config {
        bind_address: "127.0.0.1:0".to_string(),
        log_level: "info".to_string(),
        database_url: "postgres://unused".to_string(),
        database_max_connections: 1,
        facebook_page_id: "page-id".to_string(),
        facebook_page_access_token: "page-token".to_string(),
        facebook_app_id: "app-id".to_string(),
        facebook_app_secret: "app-secret".to_string(),
        facebook_webhook_verify_token: "verify-token".to_string(),
        instagram_ig_user_id: "ig-user".to_string(),
        instagram_webhook_verify_token: "ig-token".to_string(),
        customer_service_url: "http://customer".to_string(),
        message_service_url: "http://message".to_string(),
        mattermost_url: "http://mattermost".to_string(),
        mattermost_username: "admin".to_string(),
        mattermost_password: Some("password".to_string()),
        mattermost_database_url: None,
        mattermost_database_max_connections: 1,
        mattermost_bypass_mode: mode,
        mm_admin_api_token: token.map(str::to_string),
        rate_limit_warning_threshold: 80.0,
        rate_limit_critical_threshold: 95.0,
        poll_interval_secs: 0,
        minio_endpoint: "http://minio".to_string(),
        minio_access_key: "minio".to_string(),
        minio_secret_key: "minio-secret".to_string(),
        minio_bucket: "bucket".to_string(),
        minio_presigned_ttl_secs: 60,
    }
}

fn test_app(mode: BypassMode, token: Option<&str>) -> axum::Router {
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect_lazy("postgres://unused")
        .unwrap();
    let config = test_config(mode, token);
    let mattermost_client = MattermostClient::new("http://mattermost", "admin", Some("password"));
    let mattermost_ops = MattermostOps::new(
        pool.clone(),
        mattermost_client.clone(),
        None,
        config.mattermost_bypass_mode,
    );

    create_app(AppState {
        pool,
        config,
        customer_client: CustomerServiceClient::new("http://customer"),
        message_client: MessageServiceClient::new("http://message"),
        mattermost_client,
        mattermost_db: None,
        mattermost_ops,
        minio: None,
        conversation_id_cache: Arc::new(RwLock::new(HashMap::new())),
    })
}

async fn request_json(
    app: axum::Router,
    method: Method,
    uri: &str,
    token: Option<&str>,
    body: Option<&str>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(token) = token {
        builder = builder.header("Authorization", format!("Bearer {token}"));
    }
    if body.is_some() {
        builder = builder.header("Content-Type", "application/json");
    }

    let body = body.map_or_else(Body::empty, |value| Body::from(value.to_string()));
    let response = app.oneshot(builder.body(body).unwrap()).await.unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, json)
}

#[tokio::test]
async fn admin_health_requires_configured_token() {
    let (status, json) = request_json(
        test_app(BypassMode::Off, None),
        Method::GET,
        "/api/mm-admin/health",
        None,
        None,
    )
    .await;

    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(json["error"], "MM_ADMIN_API_TOKEN is not configured");
}

#[tokio::test]
async fn admin_health_rejects_missing_wrong_and_malformed_bearer() {
    for token in [None, Some("wrong-token"), Some("test-admin-token ")] {
        let (status, json) = request_json(
            test_app(BypassMode::Off, Some("test-admin-token")),
            Method::GET,
            "/api/mm-admin/health",
            token,
            None,
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(json["error"], "invalid or missing bearer token");
    }
}

#[tokio::test]
async fn admin_health_allows_valid_token_in_off_mode() {
    let (status, json) = request_json(
        test_app(BypassMode::Off, Some("test-admin-token")),
        Method::GET,
        "/api/mm-admin/health",
        Some("test-admin-token"),
        None,
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["mode"], "off");
    assert_eq!(json["mm_db"], false);
    assert_eq!(json["schema_version"], Value::Null);
}

#[tokio::test]
async fn mutating_admin_endpoints_reject_off_and_shadow_modes_before_side_effects() {
    let cases = [
        (
            Method::DELETE,
            "/api/mm-admin/channels/channel-id/posts",
            None,
        ),
        (
            Method::POST,
            "/api/mm-admin/channels/channel-id/archive",
            None,
        ),
        (
            Method::POST,
            "/api/mm-admin/channels/channel-id/unarchive",
            None,
        ),
        (
            Method::POST,
            "/api/mm-admin/dm",
            Some(r#"{"from_user_id":"u1","to_user_id":"u2","message":"hello"}"#),
        ),
    ];

    for mode in [BypassMode::Off, BypassMode::Shadow] {
        for (method, uri, body) in &cases {
            let (status, json) = request_json(
                test_app(mode, Some("test-admin-token")),
                method.clone(),
                uri,
                Some("test-admin-token"),
                *body,
            )
            .await;
            assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{method} {uri}");
            assert_eq!(json["error"], "Mattermost bypass mode is not enabled");
        }
    }
}

#[tokio::test]
async fn malformed_dm_json_is_rejected_before_handler_runs() {
    let (status, json) = request_json(
        test_app(BypassMode::Enabled, Some("test-admin-token")),
        Method::POST,
        "/api/mm-admin/dm",
        Some("test-admin-token"),
        Some(r#"{"from_user_id":"u1","message":"missing target"}"#),
    )
    .await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(json, Value::Null);
}

#[tokio::test]
async fn hostile_channel_id_is_data_not_route_escape() {
    let (status, json) = request_json(
        test_app(BypassMode::Enabled, Some("test-admin-token")),
        Method::DELETE,
        "/api/mm-admin/channels/%27%3Bdrop%20table%20posts%3B--/posts",
        Some("test-admin-token"),
        None,
    )
    .await;

    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(!json["error"].as_str().unwrap_or_default().is_empty());
}
