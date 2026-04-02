//! Integration tests for Customer Service
//!
//! These tests verify the complete functionality of the Customer Service API
//! including database operations and HTTP endpoints.

mod common;

use axum::{
    body::Body,
    http::{Method, Request, StatusCode},
    Router,
};
use customer_service::{
    config::Config,
    create_app, db,
    models::{CreateCustomerRequest, CustomerResponse, UpdateCustomerRequest},
    AppState, PgPool,
};
use tower::ServiceExt; // for oneshot
use uuid::Uuid;

use common::{cleanup_test_db, setup_test_db};

/// Test helper to create a test app with database
async fn create_test_app() -> (Router, PgPool) {
    let pool = setup_test_db().await;
    cleanup_test_db(&pool).await;

    let config = Config::default();
    let state = AppState {
        pool: pool.clone(),
        config,
    };
    let app = create_app(state);

    (app, pool)
}

#[tokio::test]
async fn test_health_check() {
    let (app, pool) = create_test_app().await;

    let response = app
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["status"], "healthy");
    assert_eq!(json["service"], "customer-service");

    pool.close().await;
}

#[tokio::test]
async fn test_create_customer() {
    let (app, pool) = create_test_app().await;

    let request = CreateCustomerRequest {
        platform_user_id: "test_user_123".to_string(),
        platform: "facebook".to_string(),
        name: Some("Test User".to_string()),
        phone: None,
    };

    let response = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/customers")
                .header("Content-Type", "application/json")
                .body(Body::from(serde_json::to_string(&request).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::CREATED);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let customer: CustomerResponse = serde_json::from_slice(&body).unwrap();

    assert_eq!(customer.platform_user_id, "test_user_123");
    assert_eq!(customer.platform, "facebook");
    assert_eq!(customer.name, Some("Test User".to_string()));

    pool.close().await;
}

#[tokio::test]
async fn test_create_customer_idempotent() {
    let (app, pool) = create_test_app().await;

    let request = CreateCustomerRequest {
        platform_user_id: "idempotent_user".to_string(),
        platform: "facebook".to_string(),
        name: Some("Original Name".to_string()),
        phone: None,
    };

    // First request - should create
    let response1 = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/customers")
                .header("Content-Type", "application/json")
                .body(Body::from(serde_json::to_string(&request).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response1.status(), StatusCode::CREATED);
    let body1 = axum::body::to_bytes(response1.into_body(), usize::MAX)
        .await
        .unwrap();
    let customer1: CustomerResponse = serde_json::from_slice(&body1).unwrap();

    // Second request with same platform_user_id - should return existing
    let response2 = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/customers")
                .header("Content-Type", "application/json")
                .body(Body::from(serde_json::to_string(&request).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response2.status(), StatusCode::CREATED);
    let body2 = axum::body::to_bytes(response2.into_body(), usize::MAX)
        .await
        .unwrap();
    let customer2: CustomerResponse = serde_json::from_slice(&body2).unwrap();

    // Should return the same customer
    assert_eq!(customer1.id, customer2.id);
    assert_eq!(customer1.platform_user_id, customer2.platform_user_id);

    pool.close().await;
}

#[tokio::test]
async fn test_get_customer_by_id() {
    let (app, pool) = create_test_app().await;

    // First create a customer
    let customer = db::get_or_create_customer(&pool, "get_test_user", "facebook", Some("Get Test"))
        .await
        .unwrap();

    // Then get by ID
    let response = app
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri(format!("/api/customers/{}", customer.id))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let retrieved: CustomerResponse = serde_json::from_slice(&body).unwrap();

    assert_eq!(retrieved.id, customer.id);
    assert_eq!(retrieved.platform_user_id, "get_test_user");

    pool.close().await;
}

#[tokio::test]
async fn test_get_customer_by_id_not_found() {
    let (app, pool) = create_test_app().await;

    let non_existent_id = Uuid::new_v4();

    let response = app
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri(format!("/api/customers/{}", non_existent_id))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    pool.close().await;
}

#[tokio::test]
async fn test_get_customer_by_platform() {
    let (app, pool) = create_test_app().await;

    // Create a customer
    let customer = db::get_or_create_customer(
        &pool,
        "platform_test_user",
        "facebook",
        Some("Platform Test"),
    )
    .await
    .unwrap();

    // Get by platform
    let response = app
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/api/customers/platform/facebook/platform_test_user")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let retrieved: CustomerResponse = serde_json::from_slice(&body).unwrap();

    assert_eq!(retrieved.id, customer.id);
    assert_eq!(retrieved.platform_user_id, "platform_test_user");
    assert_eq!(retrieved.platform, "facebook");

    pool.close().await;
}

#[tokio::test]
async fn test_update_customer() {
    let (app, pool) = create_test_app().await;

    // Create a customer
    let customer =
        db::get_or_create_customer(&pool, "update_test_user", "facebook", Some("Original Name"))
            .await
            .unwrap();

    // Update the customer
    let update_request = UpdateCustomerRequest {
        name: Some("Updated Name".to_string()),
        phone: Some("+1234567890".to_string()),
    };

    let response = app
        .oneshot(
            Request::builder()
                .method(Method::PUT)
                .uri(format!("/api/customers/{}", customer.id))
                .header("Content-Type", "application/json")
                .body(Body::from(serde_json::to_string(&update_request).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let updated: CustomerResponse = serde_json::from_slice(&body).unwrap();

    assert_eq!(updated.id, customer.id);
    assert_eq!(updated.name, Some("Updated Name".to_string()));
    assert_eq!(updated.phone, Some("+1234567890".to_string()));

    pool.close().await;
}

#[tokio::test]
async fn test_list_customers() {
    let (app, pool) = create_test_app().await;

    // Create multiple customers
    db::get_or_create_customer(&pool, "list_user_1", "facebook", Some("User 1"))
        .await
        .unwrap();
    db::get_or_create_customer(&pool, "list_user_2", "facebook", Some("User 2"))
        .await
        .unwrap();
    db::get_or_create_customer(&pool, "list_user_3", "zalo", Some("User 3"))
        .await
        .unwrap();

    // List all customers
    let response = app
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/api/customers")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let customers: Vec<CustomerResponse> = serde_json::from_slice(&body).unwrap();

    let user_ids: Vec<&str> = customers
        .iter()
        .map(|c| c.platform_user_id.as_str())
        .collect();
    let created_ids = ["list_user_1", "list_user_2", "list_user_3"];
    let found_count = user_ids
        .iter()
        .filter(|id| created_ids.contains(id))
        .count();
    assert!(
        found_count >= 3,
        "Expected to find all 3 created customers, found {}. Got user_ids: {:?}",
        found_count, user_ids
    );

    pool.close().await;
}

#[tokio::test]
async fn test_list_customers_by_platform() {
    let (app, pool) = create_test_app().await;

    // Create customers on different platforms
    db::get_or_create_customer(&pool, "platform_filter_1", "facebook", Some("FB User"))
        .await
        .unwrap();
    db::get_or_create_customer(&pool, "platform_filter_2", "zalo", Some("Zalo User"))
        .await
        .unwrap();

    // List only Facebook customers
    let response = app
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/api/customers?platform=facebook")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let customers: Vec<CustomerResponse> = serde_json::from_slice(&body).unwrap();

    // All returned customers should be from Facebook platform
    for customer in &customers {
        assert_eq!(customer.platform, "facebook");
    }

    pool.close().await;
}

#[tokio::test]
async fn test_customer_stats() {
    let (app, pool) = create_test_app().await;

    // Create customers on different platforms
    db::get_or_create_customer(&pool, "stats_user_1", "facebook", Some("FB User 1"))
        .await
        .unwrap();
    db::get_or_create_customer(&pool, "stats_user_2", "facebook", Some("FB User 2"))
        .await
        .unwrap();
    db::get_or_create_customer(&pool, "stats_user_3", "zalo", Some("Zalo User"))
        .await
        .unwrap();

    // Get stats
    let response = app
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/api/customers/stats")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let stats: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert!(stats["total"].as_i64().unwrap() >= 3);
    assert!(stats["by_platform"]["facebook"].as_i64().unwrap() >= 2);
    assert!(stats["by_platform"]["zalo"].as_i64().unwrap() >= 1);

    pool.close().await;
}
