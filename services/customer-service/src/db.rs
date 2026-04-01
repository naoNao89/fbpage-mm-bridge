use crate::models::{Customer, ListCustomersQuery};
use anyhow::Result;
use sqlx::{postgres::PgPoolOptions, PgPool};
use uuid::Uuid;

/// Create a database connection pool
pub async fn create_pool(database_url: &str) -> Result<PgPool> {
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(database_url)
        .await?;

    Ok(pool)
}

/// Get or create a customer by platform user ID
///
/// This is the primary method for customer resolution. It first attempts to find
/// an existing customer by platform_user_id and platform. If not found, it creates
/// a new customer.
pub async fn get_or_create_customer(
    pool: &PgPool,
    platform_user_id: &str,
    platform: &str,
    name: Option<&str>,
) -> Result<Customer> {
    // Try to find existing customer
    let existing = sqlx::query_as::<_, Customer>(
        "SELECT * FROM customers WHERE platform_user_id = $1 AND platform = $2",
    )
    .bind(platform_user_id)
    .bind(platform)
    .fetch_optional(pool)
    .await?;

    if let Some(customer) = existing {
        return Ok(customer);
    }

    // Create new customer
    let id = Uuid::new_v4();
    let customer = sqlx::query_as::<_, Customer>(
        r#"
        INSERT INTO customers (id, platform_user_id, platform, name, created_at)
        VALUES ($1, $2, $3, $4, NOW())
        RETURNING *
        "#,
    )
    .bind(id)
    .bind(platform_user_id)
    .bind(platform)
    .bind(name)
    .fetch_one(pool)
    .await?;

    Ok(customer)
}

/// Get a customer by ID
pub async fn get_customer_by_id(pool: &PgPool, id: Uuid) -> Result<Option<Customer>> {
    let customer = sqlx::query_as::<_, Customer>("SELECT * FROM customers WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await?;

    Ok(customer)
}

/// Get a customer by platform user ID and platform
pub async fn get_customer_by_platform_id(
    pool: &PgPool,
    platform_user_id: &str,
    platform: &str,
) -> Result<Option<Customer>> {
    let customer = sqlx::query_as::<_, Customer>(
        "SELECT * FROM customers WHERE platform_user_id = $1 AND platform = $2",
    )
    .bind(platform_user_id)
    .bind(platform)
    .fetch_optional(pool)
    .await?;

    Ok(customer)
}

/// Update a customer's profile information
pub async fn update_customer(
    pool: &PgPool,
    id: Uuid,
    name: Option<&str>,
    phone: Option<&str>,
) -> Result<Option<Customer>> {
    let customer = sqlx::query_as::<_, Customer>(
        r#"
        UPDATE customers
        SET name = COALESCE($2, name),
            phone = COALESCE($3, phone)
        WHERE id = $1
        RETURNING *
        "#,
    )
    .bind(id)
    .bind(name)
    .bind(phone)
    .fetch_optional(pool)
    .await?;

    Ok(customer)
}

/// List customers with optional filtering and pagination
pub async fn list_customers(pool: &PgPool, query: &ListCustomersQuery) -> Result<Vec<Customer>> {
    let mut query_builder = sqlx::QueryBuilder::new("SELECT * FROM customers");

    if let Some(platform) = &query.platform {
        query_builder.push(" WHERE platform = ");
        query_builder.push_bind(platform);
    }

    query_builder.push(" ORDER BY created_at DESC");

    if let Some(limit) = query.limit {
        query_builder.push(" LIMIT ");
        query_builder.push_bind(limit);
    }

    if let Some(offset) = query.offset {
        query_builder.push(" OFFSET ");
        query_builder.push_bind(offset);
    }

    let customers = query_builder
        .build_query_as::<Customer>()
        .fetch_all(pool)
        .await?;

    Ok(customers)
}

/// Get customers without channel mappings (for Mattermost channel creation)
pub async fn get_customers_without_mapping(pool: &PgPool) -> Result<Vec<Customer>> {
    let customers = sqlx::query_as::<_, Customer>(
        r#"
        SELECT c.* FROM customers c
        LEFT JOIN channel_mappings cm ON c.id = cm.customer_id
        WHERE cm.id IS NULL
        ORDER BY c.created_at DESC
        "#,
    )
    .fetch_all(pool)
    .await?;

    Ok(customers)
}

/// Count total customers
pub async fn count_customers(pool: &PgPool) -> Result<i64> {
    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM customers")
        .fetch_one(pool)
        .await?;

    Ok(count.0)
}

/// Count customers by platform
pub async fn count_customers_by_platform(pool: &PgPool, platform: &str) -> Result<i64> {
    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM customers WHERE platform = $1")
        .bind(platform)
        .fetch_one(pool)
        .await?;

    Ok(count.0)
}

#[cfg(test)]
mod tests {

    #[test]
    fn test_build_query_with_platform() {
        // This is just a compile-time check that the module builds correctly
        // Integration tests will test the actual database operations
    }
}
