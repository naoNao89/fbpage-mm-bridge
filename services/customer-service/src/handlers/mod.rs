use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use uuid::Uuid;

use crate::db;
use crate::models::{
    CreateCustomerRequest, CustomerResponse, ListCustomersQuery, UpdateCustomerRequest,
};
use crate::AppState;

/// Health check endpoint
pub async fn health_check() -> impl IntoResponse {
    Json(serde_json::json!({ "status": "healthy", "service": "customer-service" }))
}

/// Get customer by ID
///
/// GET /api/customers/:id
pub async fn get_customer(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    match db::get_customer_by_id(&state.pool, id).await {
        Ok(Some(customer)) => {
            let response: CustomerResponse = customer.into();
            (StatusCode::OK, Json(response)).into_response()
        }
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": "Customer not found",
                "id": id.to_string()
            })),
        )
            .into_response(),
        Err(e) => {
            tracing::error!("Failed to get customer {}: {}", id, e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": "Failed to get customer",
                    "details": e.to_string()
                })),
            )
                .into_response()
        }
    }
}

/// Get customer by platform user ID
///
/// GET /api/customers/platform/:platform/:user_id
pub async fn get_customer_by_platform(
    State(state): State<AppState>,
    Path((platform, user_id)): Path<(String, String)>,
) -> impl IntoResponse {
    match db::get_customer_by_platform_id(&state.pool, &user_id, &platform).await {
        Ok(Some(customer)) => {
            let response: CustomerResponse = customer.into();
            (StatusCode::OK, Json(response)).into_response()
        }
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": "Customer not found",
                "platform": platform,
                "user_id": user_id
            })),
        )
            .into_response(),
        Err(e) => {
            tracing::error!("Failed to get customer {}/{}: {}", platform, user_id, e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": "Failed to get customer",
                    "details": e.to_string()
                })),
            )
                .into_response()
        }
    }
}

/// Create or get customer
///
/// POST /api/customers
///
/// This endpoint is idempotent - if a customer with the same platform_user_id
/// and platform already exists, it returns the existing customer.
pub async fn create_or_get_customer(
    State(state): State<AppState>,
    Json(payload): Json<CreateCustomerRequest>,
) -> impl IntoResponse {
    match db::get_or_create_customer(
        &state.pool,
        &payload.platform_user_id,
        &payload.platform,
        payload.name.as_deref(),
    )
    .await
    {
        Ok(customer) => {
            let response: CustomerResponse = customer.into();
            (StatusCode::CREATED, Json(response)).into_response()
        }
        Err(e) => {
            tracing::error!("Failed to create customer: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": "Failed to create customer",
                    "details": e.to_string()
                })),
            )
                .into_response()
        }
    }
}

/// Update customer profile
///
/// PUT /api/customers/:id
pub async fn update_customer(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(payload): Json<UpdateCustomerRequest>,
) -> impl IntoResponse {
    match db::update_customer(
        &state.pool,
        id,
        payload.name.as_deref(),
        payload.phone.as_deref(),
    )
    .await
    {
        Ok(Some(customer)) => {
            let response: CustomerResponse = customer.into();
            (StatusCode::OK, Json(response)).into_response()
        }
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": "Customer not found",
                "id": id.to_string()
            })),
        )
            .into_response(),
        Err(e) => {
            tracing::error!("Failed to update customer {}: {}", id, e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": "Failed to update customer",
                    "details": e.to_string()
                })),
            )
                .into_response()
        }
    }
}

/// List customers with optional filtering
///
/// GET /api/customers?platform=facebook&limit=50&offset=0
pub async fn list_customers(
    State(state): State<AppState>,
    Query(query): Query<ListCustomersQuery>,
) -> impl IntoResponse {
    match db::list_customers(&state.pool, &query).await {
        Ok(customers) => {
            let responses: Vec<CustomerResponse> =
                customers.into_iter().map(|c| c.into()).collect();
            (StatusCode::OK, Json(responses)).into_response()
        }
        Err(e) => {
            tracing::error!("Failed to list customers: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": "Failed to list customers",
                    "details": e.to_string()
                })),
            )
                .into_response()
        }
    }
}

/// Get customers without channel mappings
///
/// GET /api/customers/without-mapping
pub async fn get_customers_without_mapping(State(state): State<AppState>) -> impl IntoResponse {
    match db::get_customers_without_mapping(&state.pool).await {
        Ok(customers) => {
            let responses: Vec<CustomerResponse> =
                customers.into_iter().map(|c| c.into()).collect();
            (StatusCode::OK, Json(responses)).into_response()
        }
        Err(e) => {
            tracing::error!("Failed to get customers without mapping: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": "Failed to get customers without mapping",
                    "details": e.to_string()
                })),
            )
                .into_response()
        }
    }
}

/// Get customer statistics
///
/// GET /api/customers/stats
pub async fn get_customer_stats(State(state): State<AppState>) -> impl IntoResponse {
    match db::count_customers(&state.pool).await {
        Ok(total) => {
            // Get counts by platform
            let facebook_count = db::count_customers_by_platform(&state.pool, "facebook")
                .await
                .unwrap_or(0);
            let zalo_count = db::count_customers_by_platform(&state.pool, "zalo")
                .await
                .unwrap_or(0);

            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "total": total,
                    "by_platform": {
                        "facebook": facebook_count,
                        "zalo": zalo_count
                    }
                })),
            )
                .into_response()
        }
        Err(e) => {
            tracing::error!("Failed to get customer stats: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": "Failed to get customer stats",
                    "details": e.to_string()
                })),
            )
                .into_response()
        }
    }
}
