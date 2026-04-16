//! Database operations for Facebook Graph Service

use anyhow::Context;
use chrono::{DateTime, Utc};
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use tracing::info;
use uuid::Uuid;

use crate::models::{ImportJob, ImportStatusResponse};

/// Create a database connection pool
pub async fn create_pool(database_url: &str) -> anyhow::Result<PgPool> {
    let pool = PgPoolOptions::new()
        .max_connections(10)
        .connect(database_url)
        .await
        .context("Failed to connect to database")?;

    info!("Database connection pool created");
    Ok(pool)
}

// Rate Limit Operations

/// Check if an endpoint is currently rate limited
pub async fn is_rate_limited(pool: &PgPool, endpoint: &str) -> anyhow::Result<bool> {
    let result = sqlx::query_scalar::<_, bool>(
        r#"
        SELECT EXISTS(
            SELECT 1 FROM facebook_rate_limits
            WHERE endpoint = $1
            AND reset_at > NOW()
            AND calls_remaining IS NOT NULL
            AND calls_remaining <= 0
        )
        "#,
    )
    .bind(endpoint)
    .fetch_one(pool)
    .await
    .context("Failed to check rate limit status")?;

    Ok(result)
}

/// Upsert rate limit information
pub async fn upsert_rate_limit(
    pool: &PgPool,
    endpoint: &str,
    call_count: i32,
    call_limit: i32,
    reset_at: DateTime<Utc>,
    headers_json: serde_json::Value,
) -> anyhow::Result<()> {
    sqlx::query(
        r#"
        INSERT INTO facebook_rate_limits (endpoint, calls_remaining, calls_total, reset_at, last_response_headers, updated_at)
        VALUES ($1, $2, $3, $4, $5, NOW())
        ON CONFLICT (endpoint) DO UPDATE SET
            calls_remaining = EXCLUDED.calls_remaining,
            calls_total = EXCLUDED.calls_total,
            reset_at = EXCLUDED.reset_at,
            last_response_headers = EXCLUDED.last_response_headers,
            updated_at = NOW()
        "#,
    )
    .bind(endpoint)
    .bind(call_limit - call_count)
    .bind(call_limit)
    .bind(reset_at)
    .bind(headers_json)
    .execute(pool)
    .await
    .context("Failed to upsert rate limit")?;

    Ok(())
}

// Import Job Operations

/// Create a new import job
pub async fn create_import_job(pool: &PgPool, job_id: Uuid, status: &str) -> anyhow::Result<()> {
    sqlx::query(
        r#"
        INSERT INTO facebook_import_jobs (id, status, created_at, updated_at)
        VALUES ($1, $2, NOW(), NOW())
        "#,
    )
    .bind(job_id)
    .bind(status)
    .execute(pool)
    .await
    .context("Failed to create import job")?;

    Ok(())
}

/// Update import job as started
pub async fn update_import_job_started(pool: &PgPool, job_id: Uuid) -> anyhow::Result<()> {
    sqlx::query(
        r#"
        UPDATE facebook_import_jobs
        SET status = 'running', started_at = NOW(), updated_at = NOW()
        WHERE id = $1
        "#,
    )
    .bind(job_id)
    .execute(pool)
    .await
    .context("Failed to update import job")?;

    Ok(())
}

/// Update import job totals
pub async fn update_import_job_totals(
    pool: &PgPool,
    job_id: Uuid,
    total_conversations: i32,
) -> anyhow::Result<()> {
    sqlx::query(
        r#"
        UPDATE facebook_import_jobs
        SET total_conversations = $2, updated_at = NOW()
        WHERE id = $1
        "#,
    )
    .bind(job_id)
    .bind(total_conversations)
    .execute(pool)
    .await
    .context("Failed to update import job totals")?;

    Ok(())
}

/// Update import job progress
#[allow(clippy::too_many_arguments)]
pub async fn update_import_job_progress(
    pool: &PgPool,
    job_id: Uuid,
    processed: i32,
    failed: i32,
    total_messages: i32,
    stored: i32,
    skipped: i32,
) -> anyhow::Result<()> {
    sqlx::query(
        r#"
        UPDATE facebook_import_jobs
        SET processed_conversations = $2,
            failed_conversations = $3,
            total_messages = $4,
            messages_stored = $5,
            messages_skipped = $6,
            updated_at = NOW()
        WHERE id = $1
        "#,
    )
    .bind(job_id)
    .bind(processed)
    .bind(failed)
    .bind(total_messages)
    .bind(stored)
    .bind(skipped)
    .execute(pool)
    .await
    .context("Failed to update import job progress")?;

    Ok(())
}

/// Update import job as completed
#[allow(clippy::too_many_arguments)]
pub async fn update_import_job_completed(
    pool: &PgPool,
    job_id: Uuid,
    processed: i32,
    failed: i32,
    total_messages: i32,
    stored: i32,
    skipped: i32,
    status: &str,
) -> anyhow::Result<()> {
    sqlx::query(
        r#"
        UPDATE facebook_import_jobs
        SET status = $2,
            processed_conversations = $3,
            failed_conversations = $4,
            total_messages = $5,
            messages_stored = $6,
            messages_skipped = $7,
            completed_at = NOW(),
            updated_at = NOW()
        WHERE id = $1
        "#,
    )
    .bind(job_id)
    .bind(status)
    .bind(processed)
    .bind(failed)
    .bind(total_messages)
    .bind(stored)
    .bind(skipped)
    .execute(pool)
    .await
    .context("Failed to update import job")?;

    Ok(())
}

/// Update import job with error
pub async fn update_import_job_error(
    pool: &PgPool,
    job_id: Uuid,
    error: &str,
) -> anyhow::Result<()> {
    sqlx::query(
        r#"
        UPDATE facebook_import_jobs
        SET status = 'failed',
            error_message = $2,
            completed_at = NOW(),
            updated_at = NOW()
        WHERE id = $1
        "#,
    )
    .bind(job_id)
    .bind(error)
    .execute(pool)
    .await
    .context("Failed to update import job error")?;

    Ok(())
}

/// Get the latest import status
pub async fn get_latest_import_status(pool: &PgPool) -> anyhow::Result<ImportStatusResponse> {
    let job = sqlx::query_as::<_, ImportJob>(
        r#"
        SELECT * FROM facebook_import_jobs
        ORDER BY created_at DESC
        LIMIT 1
        "#,
    )
    .fetch_optional(pool)
    .await
    .context("Failed to fetch latest import job")?;

    match job {
        Some(j) => Ok(ImportStatusResponse {
            status: j.status,
            total_conversations: j.total_conversations.unwrap_or(0),
            processed_conversations: j.processed_conversations.unwrap_or(0),
            failed_conversations: j.failed_conversations.unwrap_or(0),
            total_messages: j.total_messages.unwrap_or(0),
            messages_stored: j.messages_stored.unwrap_or(0),
            messages_skipped: j.messages_skipped.unwrap_or(0),
            started_at: j.started_at,
            completed_at: j.completed_at,
        }),
        None => Ok(ImportStatusResponse {
            status: "no_jobs".to_string(),
            total_conversations: 0,
            processed_conversations: 0,
            failed_conversations: 0,
            total_messages: 0,
            messages_stored: 0,
            messages_skipped: 0,
            started_at: None,
            completed_at: None,
        }),
    }
}

// Conversation Import Operations

/// Create a conversation import record
pub async fn create_conversation_import(
    pool: &PgPool,
    id: Uuid,
    job_id: Uuid,
    conversation_id: &str,
) -> anyhow::Result<()> {
    sqlx::query(
        r#"
        INSERT INTO facebook_conversation_imports (id, job_id, conversation_id, status, created_at, updated_at)
        VALUES ($1, $2, $3, 'pending', NOW(), NOW())
        ON CONFLICT (conversation_id) DO NOTHING
        "#,
    )
    .bind(id)
    .bind(job_id)
    .bind(conversation_id)
    .execute(pool)
    .await
    .context("Failed to create conversation import")?;

    Ok(())
}

/// Update conversation import as started
pub async fn update_conversation_import_started(pool: &PgPool, id: Uuid) -> anyhow::Result<()> {
    sqlx::query(
        r#"
        UPDATE facebook_conversation_imports
        SET status = 'running', started_at = NOW(), updated_at = NOW()
        WHERE id = $1
        "#,
    )
    .bind(id)
    .execute(pool)
    .await
    .context("Failed to update conversation import")?;

    Ok(())
}

/// Update conversation import as completed
pub async fn update_conversation_import_completed(
    pool: &PgPool,
    id: Uuid,
    messages_fetched: i32,
    messages_stored: i32,
) -> anyhow::Result<()> {
    sqlx::query(
        r#"
        UPDATE facebook_conversation_imports
        SET status = 'completed',
            messages_fetched = $2,
            messages_stored = $3,
            completed_at = NOW(),
            updated_at = NOW()
        WHERE id = $1
        "#,
    )
    .bind(id)
    .bind(messages_fetched)
    .bind(messages_stored)
    .execute(pool)
    .await
    .context("Failed to update conversation import")?;

    Ok(())
}

/// Update conversation import with error
pub async fn update_conversation_import_error(
    pool: &PgPool,
    id: Uuid,
    error: &str,
) -> anyhow::Result<()> {
    sqlx::query(
        r#"
        UPDATE facebook_conversation_imports
        SET status = 'failed',
            error_message = $2,
            completed_at = NOW(),
            updated_at = NOW()
        WHERE id = $1
        "#,
    )
    .bind(id)
    .bind(error)
    .execute(pool)
    .await
    .context("Failed to update conversation import error")?;

    Ok(())
}

// Mattermost Cache Operations

/// Load all cache entries of a given key_type ('channel' or 'root') into a HashMap.
pub async fn load_mm_cache(
    pool: &PgPool,
    key_type: &str,
) -> anyhow::Result<std::collections::HashMap<String, String>> {
    let rows = sqlx::query_as::<_, (String, String)>(
        r#"
        SELECT conversation_id, value
        FROM mattermost_cache
        WHERE key_type = $1
        "#,
    )
    .bind(key_type)
    .fetch_all(pool)
    .await
    .context("Failed to load mattermost cache")?;

    Ok(rows.into_iter().collect())
}

/// Upsert a single cache entry.
pub async fn upsert_mm_cache(
    pool: &PgPool,
    key_type: &str,
    conversation_id: &str,
    value: &str,
) -> anyhow::Result<()> {
    sqlx::query(
        r#"
        INSERT INTO mattermost_cache (key_type, conversation_id, value, created_at, updated_at)
        VALUES ($1, $2, $3, NOW(), NOW())
        ON CONFLICT (key_type, conversation_id) DO UPDATE SET
            value = EXCLUDED.value,
            updated_at = NOW()
        "#,
    )
    .bind(key_type)
    .bind(conversation_id)
    .bind(value)
    .execute(pool)
    .await
    .context("Failed to upsert mattermost cache entry")?;

    Ok(())
}

/// Load a single cache entry by key_type and conversation_id.
pub async fn load_single_mm_cache(
    pool: &PgPool,
    key_type: &str,
    conversation_id: &str,
) -> anyhow::Result<Option<String>> {
    let result = sqlx::query_as::<_, (String,)>(
        r#"
        SELECT value FROM mattermost_cache
        WHERE key_type = $1 AND conversation_id = $2
        "#,
    )
    .bind(key_type)
    .bind(conversation_id)
    .fetch_optional(pool)
    .await
    .context("Failed to load single mattermost cache entry")?;

    Ok(result.map(|(v,)| v))
}

/// Check if a message ID has been posted to Mattermost.
pub async fn is_message_posted(pool: &PgPool, external_id: &str) -> anyhow::Result<bool> {
    let result = sqlx::query_scalar::<_, bool>(
        r#"
        SELECT EXISTS(
            SELECT 1 FROM posted_message_ids WHERE external_id = $1
        )
        "#,
    )
    .bind(external_id)
    .fetch_one(pool)
    .await
    .context("Failed to check if message was posted")?;

    Ok(result)
}

/// Mark a message ID as posted to Mattermost.
pub async fn mark_message_posted(
    pool: &PgPool,
    external_id: &str,
    conversation_id: &str,
    mattermost_post_id: &str,
) -> anyhow::Result<bool> {
    let result = sqlx::query_scalar::<_, bool>(
        r#"
        INSERT INTO posted_message_ids (external_id, conversation_id, mattermost_post_id)
        VALUES ($1, $2, $3)
        ON CONFLICT (external_id) DO NOTHING
        RETURNING TRUE
        "#,
    )
    .bind(external_id)
    .bind(conversation_id)
    .bind(mattermost_post_id)
    .fetch_optional(pool)
    .await
    .context("Failed to mark message as posted")?;

    Ok(result.is_some())
}
