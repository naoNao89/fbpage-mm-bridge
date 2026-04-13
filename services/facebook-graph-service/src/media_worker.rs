use std::time::Duration;
use tracing::{info, warn};

use crate::AppState;

pub async fn run_media_worker(state: AppState, interval_secs: u64) {
    info!("Media download worker started (interval: {interval_secs}s)");

    loop {
        tokio::time::sleep(Duration::from_secs(interval_secs)).await;

        if let Err(e) = process_stale_attachments(&state).await {
            warn!("Media worker cycle failed: {e}");
        }
    }
}

async fn process_stale_attachments(state: &AppState) -> anyhow::Result<()> {
    let minio = match &state.minio {
        Some(m) => m,
        None => return Ok(()),
    };

    let rows = sqlx::query_as::<_, (uuid::Uuid, String, String, String, Option<String>)>(
        r#"
        SELECT id, cdn_url, attachment_type, conversation_id, message_external_id
        FROM media_download_jobs
        WHERE status = 'pending'
          AND retry_count < 3
        ORDER BY created_at
        LIMIT 50
        "#,
    )
    .fetch_all(&state.pool)
    .await?;

    if rows.is_empty() {
        return Ok(());
    }

    info!("Media worker: processing {} pending jobs", rows.len());

    let cdn_client = match crate::media::build_cdn_client() {
        Ok(c) => c,
        Err(e) => {
            warn!("Media worker: failed to build CDN client: {e}");
            return Ok(());
        }
    };

    for (job_id, cdn_url, attachment_type, _conversation_id, message_external_id) in &rows {
        mark_job_status(&state.pool, *job_id, "downloading").await?;

        match crate::media::download_from_cdn(&cdn_client, cdn_url).await {
            Ok((data, detected_ct)) => {
                let key = crate::storage::build_media_key(
                    attachment_type,
                    &state.config.facebook_page_id,
                    message_external_id.as_deref().unwrap_or("unknown"),
                    &format!("attachment-{}", job_id),
                );

                match minio.upload_media(&key, data, &detected_ct).await {
                    Ok(_etag) => {
                        mark_job_completed(&state.pool, *job_id, &key).await?;
                        info!(
                            "Media worker: downloaded & uploaded {} for job {job_id}",
                            attachment_type
                        );
                    }
                    Err(e) => {
                        warn!("Media worker: MinIO upload failed for job {job_id}: {e}");
                        mark_job_failed(&state.pool, *job_id, &e.to_string()).await?;
                    }
                }
            }
            Err(e) => {
                warn!("Media worker: CDN download failed for job {job_id}: {e}");
                mark_job_failed(&state.pool, *job_id, &e.to_string()).await?;
            }
        }
    }

    Ok(())
}

async fn mark_job_status(pool: &sqlx::PgPool, id: uuid::Uuid, status: &str) -> anyhow::Result<()> {
    sqlx::query("UPDATE media_download_jobs SET status = $2, updated_at = NOW() WHERE id = $1")
        .bind(id)
        .bind(status)
        .execute(pool)
        .await?;
    Ok(())
}

async fn mark_job_completed(
    pool: &sqlx::PgPool,
    id: uuid::Uuid,
    minio_key: &str,
) -> anyhow::Result<()> {
    sqlx::query(
        "UPDATE media_download_jobs SET status = 'completed', minio_key = $2, updated_at = NOW() WHERE id = $1",
    )
    .bind(id)
    .bind(minio_key)
    .execute(pool)
    .await?;
    Ok(())
}

async fn mark_job_failed(pool: &sqlx::PgPool, id: uuid::Uuid, error: &str) -> anyhow::Result<()> {
    sqlx::query(
        r#"UPDATE media_download_jobs
        SET status = CASE WHEN retry_count >= 2 THEN 'failed' ELSE 'pending' END,
            retry_count = retry_count + 1,
            error_message = $2,
            updated_at = NOW()
        WHERE id = $1"#,
    )
    .bind(id)
    .bind(error)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn enqueue_download_job(
    pool: &sqlx::PgPool,
    message_external_id: &str,
    conversation_id: &str,
    attachment_type: &str,
    cdn_url: &str,
) -> anyhow::Result<()> {
    sqlx::query(
        r#"INSERT INTO media_download_jobs (message_external_id, conversation_id, attachment_type, cdn_url, status)
        VALUES ($1, $2, $3, $4, 'pending')
        ON CONFLICT DO NOTHING"#,
    )
    .bind(message_external_id)
    .bind(conversation_id)
    .bind(attachment_type)
    .bind(cdn_url)
    .execute(pool)
    .await?;
    Ok(())
}
