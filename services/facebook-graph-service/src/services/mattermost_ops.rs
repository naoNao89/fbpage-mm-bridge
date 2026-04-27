use crate::config::BypassMode;
use crate::services::{MattermostClient, MattermostDbClient};
use anyhow::{Context, Result};
use serde::Serialize;
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use std::time::Instant;

/// Result of a Mattermost operation that may use either REST API or DB bypass.
#[derive(Debug, Clone, Serialize)]
pub struct OperationResult<T>
where
    T: Serialize,
{
    pub result: T,
    pub path: String,
    pub duration_ms: u128,
}

/// Payload returned by bulk post deletion.
#[derive(Debug, Clone, Serialize)]
pub struct DeletePostsResult {
    pub deleted: u64,
}

/// Payload returned by direct DM send.
#[derive(Debug, Clone, Serialize)]
pub struct SendDmResult {
    pub post_id: String,
    pub channel_id: String,
}

/// Policy façade for Mattermost operations that may need to bypass REST API limits.
///
/// All handlers should call this layer instead of choosing between
/// `MattermostClient` and `MattermostDbClient` directly. This keeps bypass policy,
/// audit logging, and idempotency in one place.
#[derive(Clone)]
pub struct MattermostOps {
    pool: PgPool,
    api: MattermostClient,
    db: Option<MattermostDbClient>,
    mode: BypassMode,
}

struct AuditRecord<'a> {
    op: &'a str,
    params_hash: &'a str,
    path_taken: &'a str,
    fallback_reason: Option<&'a str>,
    status: &'a str,
    idempotency_key: Option<&'a str>,
    result_id: Option<&'a str>,
    duration_ms: u128,
}

impl MattermostOps {
    pub fn new(
        pool: PgPool,
        api: MattermostClient,
        db: Option<MattermostDbClient>,
        mode: BypassMode,
    ) -> Self {
        Self {
            pool,
            api,
            db,
            mode,
        }
    }

    pub fn mode(&self) -> BypassMode {
        self.mode
    }

    pub fn has_db(&self) -> bool {
        self.db.is_some()
    }

    pub async fn schema_version(&self) -> Result<Option<String>> {
        match &self.db {
            Some(db) => db.schema_version().await,
            None => Ok(None),
        }
    }

    pub async fn delete_all_posts_in_channel(
        &self,
        channel_id: &str,
    ) -> Result<OperationResult<DeletePostsResult>> {
        let started = Instant::now();
        let params_hash = hash_params(["channel_id", channel_id]);

        let outcome = match (self.mode, &self.db) {
            (BypassMode::Enabled, Some(db)) => {
                let deleted = db.soft_delete_all_posts_in_channel(channel_id).await?;
                self.api.cache_nudge().await;
                (deleted, "db".to_string(), None)
            }
            (BypassMode::Shadow, Some(_)) => {
                let deleted = u64::from(self.api.delete_all_posts_in_channel(channel_id).await?);
                tracing::info!(
                    "Mattermost bypass shadow: delete_all_posts_in_channel used api path for {channel_id}; DB delete not executed"
                );
                (deleted, "api".to_string(), Some("shadow".to_string()))
            }
            _ => {
                let deleted = u64::from(self.api.delete_all_posts_in_channel(channel_id).await?);
                (deleted, "api".to_string(), None)
            }
        };

        let duration_ms = started.elapsed().as_millis();
        self.audit(AuditRecord {
            op: "delete_all_posts_in_channel",
            params_hash: &params_hash,
            path_taken: &outcome.1,
            fallback_reason: outcome.2.as_deref(),
            status: "success",
            idempotency_key: None,
            result_id: None,
            duration_ms,
        })
        .await;

        Ok(OperationResult {
            result: DeletePostsResult { deleted: outcome.0 },
            path: outcome.1,
            duration_ms,
        })
    }

    pub async fn send_dm(
        &self,
        from_user_id: &str,
        to_user_id: &str,
        message: &str,
        idempotency_key: Option<&str>,
    ) -> Result<OperationResult<SendDmResult>> {
        let started = Instant::now();
        let params_hash = hash_params([from_user_id, to_user_id, message]);

        if let Some(key) = idempotency_key {
            if let Some((result_id, _)) = self.find_idempotent("send_dm", key, &params_hash).await?
            {
                let (post_id, channel_id) = parse_send_dm_result_id(&result_id)?;
                return Ok(OperationResult {
                    result: SendDmResult {
                        post_id,
                        channel_id,
                    },
                    path: "audit".to_string(),
                    duration_ms: started.elapsed().as_millis(),
                });
            }
        }

        let Some(db) = &self.db else {
            anyhow::bail!("Mattermost DB client is not configured");
        };
        if self.mode != BypassMode::Enabled {
            anyhow::bail!("Mattermost DB bypass is not enabled");
        }

        let (post_id, channel_id) = db.send_bot_dm(from_user_id, to_user_id, message).await?;
        self.api.cache_nudge().await;

        let result_id = format!("{post_id}:{channel_id}");
        let duration_ms = started.elapsed().as_millis();
        self.audit(AuditRecord {
            op: "send_dm",
            params_hash: &params_hash,
            path_taken: "db",
            fallback_reason: None,
            status: "success",
            idempotency_key,
            result_id: Some(&result_id),
            duration_ms,
        })
        .await;

        Ok(OperationResult {
            result: SendDmResult {
                post_id,
                channel_id,
            },
            path: "db".to_string(),
            duration_ms,
        })
    }

    pub async fn archive_channel(&self, channel_id: &str) -> Result<OperationResult<()>> {
        let started = Instant::now();
        let params_hash = hash_params(["archive", channel_id]);
        let mut path = "api".to_string();
        let mut fallback_reason = None;

        let api_result = self.api.archive_channel(channel_id).await;
        if let Err(api_error) = api_result {
            let Some(db) = &self.db else {
                return Err(api_error);
            };
            if self.mode != BypassMode::Enabled {
                return Err(api_error);
            }
            fallback_reason = Some(api_error.to_string());
            db.archive_channel(channel_id).await?;
            self.api.cache_nudge().await;
            path = "db".to_string();
        }

        let duration_ms = started.elapsed().as_millis();
        self.audit(AuditRecord {
            op: "archive_channel",
            params_hash: &params_hash,
            path_taken: &path,
            fallback_reason: fallback_reason.as_deref(),
            status: "success",
            idempotency_key: None,
            result_id: None,
            duration_ms,
        })
        .await;

        Ok(OperationResult {
            result: (),
            path,
            duration_ms,
        })
    }

    pub async fn unarchive_channel(&self, channel_id: &str) -> Result<OperationResult<()>> {
        let started = Instant::now();
        let params_hash = hash_params(["unarchive", channel_id]);
        let mut path = "api".to_string();
        let mut fallback_reason = None;

        let api_result = self.api.unarchive_channel(channel_id).await;
        if let Err(api_error) = api_result {
            let Some(db) = &self.db else {
                return Err(api_error);
            };
            if self.mode != BypassMode::Enabled {
                return Err(api_error);
            }
            fallback_reason = Some(api_error.to_string());
            db.unarchive_channel(channel_id).await?;
            self.api.cache_nudge().await;
            path = "db".to_string();
        }

        let duration_ms = started.elapsed().as_millis();
        self.audit(AuditRecord {
            op: "unarchive_channel",
            params_hash: &params_hash,
            path_taken: &path,
            fallback_reason: fallback_reason.as_deref(),
            status: "success",
            idempotency_key: None,
            result_id: None,
            duration_ms,
        })
        .await;

        Ok(OperationResult {
            result: (),
            path,
            duration_ms,
        })
    }

    async fn find_idempotent(
        &self,
        op: &str,
        idempotency_key: &str,
        params_hash: &str,
    ) -> Result<Option<(String, String)>> {
        let row: Option<(String, String, String)> = sqlx::query_as(
            "SELECT COALESCE(result_id, ''), path_taken, params_hash
             FROM mm_bypass_audit
             WHERE op = $1
               AND idempotency_key = $2
               AND status = 'success'
             ORDER BY created_at DESC
             LIMIT 1",
        )
        .bind(op)
        .bind(idempotency_key)
        .fetch_optional(&self.pool)
        .await
        .context("Failed to check Mattermost bypass idempotency")?;

        match row {
            Some((result_id, path_taken, existing_hash)) if existing_hash == params_hash => {
                Ok(Some((result_id, path_taken)))
            }
            Some(_) => anyhow::bail!("idempotency key reused with different request payload"),
            None => Ok(None),
        }
    }

    async fn audit(&self, record: AuditRecord<'_>) {
        let duration_ms = i64::try_from(record.duration_ms).unwrap_or(i64::MAX);
        if let Err(e) = sqlx::query(
            "INSERT INTO mm_bypass_audit
               (op, params_hash, path_taken, fallback_reason, status,
                idempotency_key, result_id, duration_ms)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
             ON CONFLICT (op, idempotency_key) WHERE idempotency_key IS NOT NULL
             DO NOTHING",
        )
        .bind(record.op)
        .bind(record.params_hash)
        .bind(record.path_taken)
        .bind(record.fallback_reason)
        .bind(record.status)
        .bind(record.idempotency_key)
        .bind(record.result_id)
        .bind(duration_ms)
        .execute(&self.pool)
        .await
        {
            tracing::warn!("Failed to write Mattermost bypass audit row: {e}");
        }
    }
}

fn hash_params<const N: usize>(parts: [&str; N]) -> String {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update(part.len().to_be_bytes());
        hasher.update(part.as_bytes());
    }
    format!("{:x}", hasher.finalize())
}

fn parse_send_dm_result_id(result_id: &str) -> Result<(String, String)> {
    let Some((post_id, channel_id)) = result_id.split_once(':') else {
        anyhow::bail!("invalid send_dm idempotency result_id");
    };
    if post_id.is_empty() || channel_id.is_empty() {
        anyhow::bail!("invalid send_dm idempotency result_id");
    }
    Ok((post_id.to_string(), channel_id.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::PgPoolOptions;
    use uuid::Uuid;

    #[test]
    fn hash_params_is_stable_for_same_inputs() {
        assert_eq!(hash_params(["a", "b"]), hash_params(["a", "b"]));
        assert_ne!(hash_params(["a", "b"]), hash_params(["b", "a"]));
        assert_ne!(hash_params(["ab", "c"]), hash_params(["a", "bc"]));
        assert_eq!(hash_params(["a", "b"]).len(), 64);
    }

    async fn test_pool() -> anyhow::Result<Option<PgPool>> {
        let Ok(database_url) = std::env::var("DATABASE_URL") else {
            return Ok(None);
        };
        let pool = PgPoolOptions::new()
            .max_connections(2)
            .connect(&database_url)
            .await;
        let Ok(pool) = pool else {
            return Ok(None);
        };
        if crate::run_migrations(&pool).await.is_err() {
            return Ok(None);
        }
        Ok(Some(pool))
    }

    fn test_ops(pool: PgPool, mode: BypassMode) -> MattermostOps {
        MattermostOps::new(
            pool,
            MattermostClient::new("http://mattermost.invalid", "admin", Some("password")),
            None,
            mode,
        )
    }

    #[tokio::test]
    async fn audit_writes_expected_row_shape() -> anyhow::Result<()> {
        let Some(pool) = test_pool().await? else {
            return Ok(());
        };
        let ops = test_ops(pool.clone(), BypassMode::Off);
        let op = format!("test_audit_{}", Uuid::new_v4());
        let params_hash = hash_params(["channel_id", "abc"]);

        ops.audit(AuditRecord {
            op: &op,
            params_hash: &params_hash,
            path_taken: "api",
            fallback_reason: Some("shadow"),
            status: "success",
            idempotency_key: None,
            result_id: None,
            duration_ms: 42,
        })
        .await;

        let row: (String, String, Option<String>, String, i64) = sqlx::query_as(
            "SELECT params_hash, path_taken, fallback_reason, status, duration_ms
             FROM mm_bypass_audit
             WHERE op = $1
             ORDER BY created_at DESC
             LIMIT 1",
        )
        .bind(&op)
        .fetch_one(&pool)
        .await?;

        assert_eq!(row.0, params_hash);
        assert_eq!(row.1, "api");
        assert_eq!(row.2.as_deref(), Some("shadow"));
        assert_eq!(row.3, "success");
        assert_eq!(row.4, 42);
        Ok(())
    }

    #[tokio::test]
    async fn send_dm_replays_successful_idempotency_key_before_db_access() -> anyhow::Result<()> {
        let Some(pool) = test_pool().await? else {
            return Ok(());
        };
        let ops = test_ops(pool.clone(), BypassMode::Enabled);
        let key = format!("idem-{}", Uuid::new_v4());
        let post_id = Uuid::new_v4().to_string();
        let channel_id = Uuid::new_v4().to_string();
        let result_id = format!("{post_id}:{channel_id}");

        sqlx::query(
            "INSERT INTO mm_bypass_audit
               (op, params_hash, path_taken, status, idempotency_key, result_id, duration_ms)
             VALUES ('send_dm', $1, 'db', 'success', $2, $3, 1)",
        )
        .bind(hash_params(["from", "to", "message"]))
        .bind(&key)
        .bind(&result_id)
        .execute(&pool)
        .await?;

        let result = ops.send_dm("from", "to", "message", Some(&key)).await?;

        assert_eq!(result.path, "audit");
        assert_eq!(result.result.post_id, post_id);
        assert_eq!(result.result.channel_id, channel_id);
        Ok(())
    }

    #[tokio::test]
    async fn send_dm_rejects_reused_idempotency_key_with_different_payload() -> anyhow::Result<()> {
        let Some(pool) = test_pool().await? else {
            return Ok(());
        };
        let ops = test_ops(pool.clone(), BypassMode::Enabled);
        let key = format!("idem-mismatch-{}", Uuid::new_v4());

        sqlx::query(
            "INSERT INTO mm_bypass_audit
               (op, params_hash, path_taken, status, idempotency_key, result_id, duration_ms)
             VALUES ('send_dm', $1, 'db', 'success', $2, $3, 1)",
        )
        .bind(hash_params(["from", "to", "message"]))
        .bind(&key)
        .bind("old-post:old-channel")
        .execute(&pool)
        .await?;

        let error = ops
            .send_dm("from", "to", "different message", Some(&key))
            .await
            .unwrap_err();

        assert!(error
            .to_string()
            .contains("idempotency key reused with different request payload"));
        Ok(())
    }

    #[tokio::test]
    async fn send_dm_replays_old_idempotency_key_because_index_is_not_ttl_scoped(
    ) -> anyhow::Result<()> {
        let Some(pool) = test_pool().await? else {
            return Ok(());
        };
        let ops = test_ops(pool.clone(), BypassMode::Enabled);
        let key = format!("expired-{}", Uuid::new_v4());
        let post_id = Uuid::new_v4().to_string();
        let channel_id = Uuid::new_v4().to_string();
        let result_id = format!("{post_id}:{channel_id}");

        sqlx::query(
            "INSERT INTO mm_bypass_audit
               (op, params_hash, path_taken, status, idempotency_key, result_id, duration_ms, created_at)
             VALUES ('send_dm', $1, 'db', 'success', $2, $3, 1, NOW() - INTERVAL '25 hours')",
        )
        .bind(hash_params(["from", "to", "message"]))
        .bind(&key)
        .bind(&result_id)
        .execute(&pool)
        .await?;

        let result = ops.send_dm("from", "to", "message", Some(&key)).await?;

        assert_eq!(result.path, "audit");
        assert_eq!(result.result.post_id, post_id);
        assert_eq!(result.result.channel_id, channel_id);
        Ok(())
    }
}
