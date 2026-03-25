use crate::models::{Message, MessageStats};
use anyhow::Result;
use sqlx::{postgres::PgPoolOptions, PgPool};
use uuid::Uuid;

/// Create a database connection pool
pub async fn create_pool(database_url: &str) -> Result<PgPool> {
    let pool = PgPoolOptions::new()
        .max_connections(10)
        .connect(database_url)
        .await?;

    Ok(pool)
}

/// Save a new message
pub async fn save_message(
    pool: &PgPool,
    customer_id: Uuid,
    conversation_id: &str,
    platform: &str,
    direction: &str,
    message_text: Option<&str>,
    external_id: Option<&str>,
) -> Result<Message> {
    let id = Uuid::new_v4();
    let message = sqlx::query_as::<_, Message>(
        r#"
        INSERT INTO messages (
            id, customer_id, conversation_id, platform, direction, 
            message_text, external_id, created_at
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, NOW())
        RETURNING *
        "#,
    )
    .bind(id)
    .bind(customer_id)
    .bind(conversation_id)
    .bind(platform)
    .bind(direction)
    .bind(message_text)
    .bind(external_id)
    .fetch_one(pool)
    .await?;

    Ok(message)
}

/// Get a message by ID
pub async fn get_message_by_id(pool: &PgPool, id: Uuid) -> Result<Option<Message>> {
    let message = sqlx::query_as::<_, Message>("SELECT * FROM messages WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await?;

    Ok(message)
}

/// Get messages by customer ID
pub async fn get_messages_by_customer_id(
    pool: &PgPool,
    customer_id: Uuid,
    limit: Option<i64>,
    offset: Option<i64>,
) -> Result<Vec<Message>> {
    let messages = sqlx::query_as::<_, Message>(
        r#"
        SELECT * FROM messages 
        WHERE customer_id = $1 
        ORDER BY created_at DESC
        LIMIT $2 OFFSET $3
        "#,
    )
    .bind(customer_id)
    .bind(limit.unwrap_or(50))
    .bind(offset.unwrap_or(0))
    .fetch_all(pool)
    .await?;

    Ok(messages)
}

/// Get messages by conversation ID
pub async fn get_messages_by_conversation_id(
    pool: &PgPool,
    conversation_id: &str,
    limit: Option<i64>,
    offset: Option<i64>,
) -> Result<Vec<Message>> {
    let messages = sqlx::query_as::<_, Message>(
        r#"
        SELECT * FROM messages 
        WHERE conversation_id = $1 
        ORDER BY created_at ASC
        LIMIT $2 OFFSET $3
        "#,
    )
    .bind(conversation_id)
    .bind(limit.unwrap_or(50))
    .bind(offset.unwrap_or(0))
    .fetch_all(pool)
    .await?;

    Ok(messages)
}

/// Get unsynced messages (for Mattermost sync processing)
pub async fn get_unsynced_messages(pool: &PgPool, limit: i64) -> Result<Vec<Message>> {
    let messages = sqlx::query_as::<_, Message>(
        r#"
        SELECT * FROM messages 
        WHERE mattermost_synced_at IS NULL 
          AND mattermost_sync_error IS NULL
        ORDER BY created_at ASC
        LIMIT $1
        "#,
    )
    .bind(limit)
    .fetch_all(pool)
    .await?;

    Ok(messages)
}

/// Mark message as synced to Mattermost
pub async fn mark_message_synced(
    pool: &PgPool,
    id: Uuid,
    mattermost_channel: &str,
) -> Result<Option<Message>> {
    let message = sqlx::query_as::<_, Message>(
        r#"
        UPDATE messages
        SET mattermost_synced_at = NOW(),
            mattermost_channel = $2,
            mattermost_sync_error = NULL
        WHERE id = $1
        RETURNING *
        "#,
    )
    .bind(id)
    .bind(mattermost_channel)
    .fetch_optional(pool)
    .await?;

    Ok(message)
}

/// Mark message sync as failed
pub async fn mark_message_sync_failed(
    pool: &PgPool,
    id: Uuid,
    error: &str,
) -> Result<Option<Message>> {
    let message = sqlx::query_as::<_, Message>(
        r#"
        UPDATE messages
        SET mattermost_sync_error = $2,
            mattermost_synced_at = NULL
        WHERE id = $1
        RETURNING *
        "#,
    )
    .bind(id)
    .bind(error)
    .fetch_optional(pool)
    .await?;

    Ok(message)
}

/// Get message statistics
pub async fn get_message_stats(pool: &PgPool) -> Result<MessageStats> {
    let total: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM messages")
        .fetch_one(pool)
        .await?;

    let synced: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM messages WHERE mattermost_synced_at IS NOT NULL")
            .fetch_one(pool)
            .await?;

    let unsynced: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM messages WHERE mattermost_synced_at IS NULL AND mattermost_sync_error IS NULL"
    )
    .fetch_one(pool)
    .await?;

    let sync_failed: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM messages WHERE mattermost_sync_error IS NOT NULL")
            .fetch_one(pool)
            .await?;

    Ok(MessageStats {
        total: total.0,
        synced: synced.0,
        unsynced: unsynced.0,
        sync_failed: sync_failed.0,
    })
}

/// Check if a message with the same external_id already exists
pub async fn get_message_by_external_id(
    pool: &PgPool,
    external_id: &str,
) -> Result<Option<Message>> {
    let message = sqlx::query_as::<_, Message>("SELECT * FROM messages WHERE external_id = $1")
        .bind(external_id)
        .fetch_optional(pool)
        .await?;

    Ok(message)
}

#[cfg(test)]
mod tests {
    use crate::config::Config;

    #[test]
    fn test_config_default() {
        let config = Config::default();
        assert_eq!(config.bind_address, "0.0.0.0:3002");
        assert_eq!(config.customer_service_url, "http://localhost:3001");
    }
}
