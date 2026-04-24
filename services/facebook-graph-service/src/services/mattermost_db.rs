//! Direct PostgreSQL access to Mattermost database
//!
//! This bypasses Mattermost API restrictions by writing directly to the database.
//! Use cases:
//! - Archive/unarchive channels (API requires Enterprise license)
//! - Send DMs from bots (API has strict limitations on bot-initiated DMs)
//!
//! WARNING: Direct DB manipulation bypasses application-level safeguards.
//! Use with caution and always backup your database before operations.

use sqlx::postgres::{PgPool, PgPoolOptions};
use anyhow::{Context, Result};

/// Mattermost database client for direct operations
#[derive(Clone)]
pub struct MattermostDbClient {
    pool: PgPool,
}

impl MattermostDbClient {
    /// Connect to Mattermost database
    ///
    /// Connection string format:
    /// `postgres://user:password@host:5432/mattermost`
    pub async fn new(database_url: &str) -> Result<Self> {
        let pool = PgPoolOptions::new()
            .max_connections(5)
            .connect(database_url)
            .await
            .context("Failed to connect to Mattermost database")?;

        tracing::info!("Connected to Mattermost database");
        Ok(Self { pool })
    }

    /// Archive a channel by setting delete_at timestamp
    ///
    /// In Mattermost, archived channels have delete_at > 0
    pub async fn archive_channel(&self, channel_id: &str) -> Result<()> {
        let now = chrono::Utc::now().timestamp_millis();
        let affected = sqlx::query(
            r#"
            UPDATE channels
            SET delete_at = $1, update_at = $1
            WHERE id = $2 AND delete_at = 0
            "#,
        )
        .bind(now)
        .bind(channel_id)
        .execute(&self.pool)
        .await
        .context("Failed to archive channel")?;

        if affected.rows_affected() == 0 {
            // Check if channel exists
            let exists: Option<(String, i64)> = sqlx::query_as(
                "SELECT id, delete_at FROM channels WHERE id = $1"
            )
            .bind(channel_id)
            .fetch_optional(&self.pool)
            .await?;

            match exists {
                Some((_, delete_at)) if delete_at > 0 => {
                    tracing::info!("Channel {channel_id} is already archived");
                }
                None => {
                    return Err(anyhow::anyhow!("Channel {channel_id} not found"));
                }
                _ => {}
            }
        } else {
            tracing::info!("Archived channel {channel_id}");
        }

        Ok(())
    }

    /// Unarchive a channel by clearing delete_at
    pub async fn unarchive_channel(&self, channel_id: &str) -> Result<()> {
        let affected = sqlx::query(
            r#"
            UPDATE channels
            SET delete_at = 0, update_at = $1
            WHERE id = $2 AND delete_at > 0
            "#,
        )
        .bind(chrono::Utc::now().timestamp_millis())
        .bind(channel_id)
        .execute(&self.pool)
        .await
        .context("Failed to unarchive channel")?;

        if affected.rows_affected() == 0 {
            return Err(anyhow::anyhow!(
                "Channel {channel_id} not found or not archived"
            ));
        }

        tracing::info!("Unarchived channel {channel_id}");
        Ok(())
    }

    /// Get channel info from database
    pub async fn get_channel(&self, channel_id: &str) -> Result<Option<ChannelDbInfo>> {
        let channel = sqlx::query_as::<_, ChannelDbInfo>(
            "SELECT id, name, display_name, type, team_id, delete_at FROM channels WHERE id = $1",
        )
        .bind(channel_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(channel)
    }

    /// Find a channel by name
    pub async fn find_channel_by_name(&self, name: &str) -> Result<Option<ChannelDbInfo>> {
        let channel = sqlx::query_as::<_, ChannelDbInfo>(
            "SELECT id, name, display_name, type, team_id, delete_at FROM channels WHERE name = $1",
        )
        .bind(name)
        .fetch_optional(&self.pool)
        .await?;

        Ok(channel)
    }

    /// Create or get a DM channel between two users
    ///
    /// DM channel naming convention in Mattermost:
    /// - No team: `__userId1__userId2__` (double underscore prefix/suffix)
    /// - The IDs are sorted to ensure consistent naming
    pub async fn get_or_create_dm_channel(
        &self,
        user_id_1: &str,
        user_id_2: &str,
    ) -> Result<String> {
        // Sort user IDs to ensure consistent channel name
        let (id1, id2) = if user_id_1 < user_id_2 {
            (user_id_1, user_id_2)
        } else {
            (user_id_2, user_id_1)
        };

        let channel_name = format!("__{id1}__{id2}__");

        // Try to find existing DM channel
        let existing: Option<ChannelDbInfo> = sqlx::query_as(
            "SELECT id, name, display_name, type, team_id, delete_at
             FROM channels WHERE name = $1 AND type = 'D'",
        )
        .bind(&channel_name)
        .fetch_optional(&self.pool)
        .await?;

        if let Some(channel) = existing {
            if channel.delete_at == 0 {
                tracing::debug!("Found existing DM channel: {}", channel.id);
                return Ok(channel.id);
            } else {
                // Unarchive if archived
                self.unarchive_channel(&channel.id).await?;
                return Ok(channel.id);
            }
        }

        // Create new DM channel
        let channel_id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().timestamp_millis();

        sqlx::query(
            r#"
            INSERT INTO channels (id, name, display_name, type, team_id, create_at, update_at, delete_at, header, purpose)
            VALUES ($1, $2, 'Direct Message', 'D', NULL, $3, $3, 0, '', '')
            "#,
        )
        .bind(&channel_id)
        .bind(&channel_name)
        .bind(now)
        .execute(&self.pool)
        .await
        .context("Failed to create DM channel")?;

        tracing::info!("Created DM channel {channel_id} between {id1} and {id2}");
        Ok(channel_id)
    }

    /// Add a user to a DM channel
    pub async fn add_user_to_dm_channel(
        &self,
        channel_id: &str,
        user_id: &str,
    ) -> Result<()> {
        let now = chrono::Utc::now().timestamp_millis();

        sqlx::query(
            r#"
            INSERT INTO channelmembers (channel_id, user_id, roles, last_viewed_at, msg_count, mention_count)
            VALUES ($1, $2, 'channel_user', $3, 0, 0)
            ON CONFLICT (channel_id, user_id) DO NOTHING
            "#,
        )
        .bind(channel_id)
        .bind(user_id)
        .bind(now)
        .execute(&self.pool)
        .await
        .context("Failed to add user to DM channel")?;

        tracing::debug!("Added user {user_id} to DM channel {channel_id}");
        Ok(())
    }

    /// Send a message directly to a channel via database
    ///
    /// This bypasses all API restrictions and webhook limitations.
    pub async fn send_message(
        &self,
        channel_id: &str,
        user_id: &str,
        message: &str,
    ) -> Result<String> {
        let post_id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().timestamp_millis();

        sqlx::query(
            r#"
            INSERT INTO posts (id, channel_id, user_id, message, create_at, update_at, delete_at, root_id, props)
            VALUES ($1, $2, $3, $4, $5, $5, 0, NULL, '{}')
            "#,
        )
        .bind(&post_id)
        .bind(channel_id)
        .bind(user_id)
        .bind(message)
        .bind(now)
        .execute(&self.pool)
        .await
        .context("Failed to insert post")?;

        sqlx::query(
            "UPDATE channels SET last_post_at = $1 WHERE id = $2",
        )
        .bind(now)
        .bind(channel_id)
        .execute(&self.pool)
        .await
        .ok();

        tracing::info!("Sent message to channel {channel_id}, post_id: {post_id}");
        Ok(post_id)
    }

    /// Send a DM from bot to user directly via database
    ///
    /// This is the primary use case - bots cannot send DMs via API in Team Edition.
    pub async fn send_bot_dm(
        &self,
        bot_user_id: &str,
        target_user_id: &str,
        message: &str,
    ) -> Result<String> {
        // Get or create DM channel
        let channel_id = self.get_or_create_dm_channel(bot_user_id, target_user_id).await?;

        // Add bot to channel if not already member
        self.add_user_to_dm_channel(&channel_id, bot_user_id).await?;

        // Add target user to channel if not already member
        self.add_user_to_dm_channel(&channel_id, target_user_id).await?;

        // Send the message as the bot
        let post_id = self.send_message(&channel_id, bot_user_id, message).await?;

        tracing::info!(
            "Bot {bot_user_id} sent DM to {target_user_id}, post_id: {post_id}"
        );
        Ok(post_id)
    }

    /// Get user ID by username
    pub async fn get_user_id_by_username(&self, username: &str) -> Result<Option<String>> {
        let user: Option<(String,)> = sqlx::query_as(
            "SELECT id FROM users WHERE username = $1",
        )
        .bind(username)
        .fetch_optional(&self.pool)
        .await?;

        Ok(user.map(|(id,)| id))
    }

    /// List all archived channels
    pub async fn list_archived_channels(&self) -> Result<Vec<ChannelDbInfo>> {
        let channels = sqlx::query_as::<_, ChannelDbInfo>(
            "SELECT id, name, display_name, type, team_id, delete_at
             FROM channels WHERE delete_at > 0 ORDER BY delete_at DESC",
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(channels)
    }

    /// List all active channels (optionally filtered by prefix)
    pub async fn list_channels(&self, prefix: Option<&str>) -> Result<Vec<ChannelDbInfo>> {
        let channels = match prefix {
            Some(p) => {
                sqlx::query_as::<_, ChannelDbInfo>(
                    "SELECT id, name, display_name, type, team_id, delete_at
                     FROM channels WHERE delete_at = 0 AND name LIKE $1 ORDER BY name",
                )
                .bind(format!("{p}%"))
                .fetch_all(&self.pool)
                .await?
            }
            None => {
                sqlx::query_as::<_, ChannelDbInfo>(
                    "SELECT id, name, display_name, type, team_id, delete_at
                     FROM channels WHERE delete_at = 0 ORDER BY name",
                )
                .fetch_all(&self.pool)
                .await?
            }
        };

        Ok(channels)
    }
}

/// Channel information from database
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ChannelDbInfo {
    pub id: String,
    pub name: String,
    pub display_name: String,
    pub r#type: String,  // O=public, P=private, D=direct
    pub team_id: Option<String>,
    pub delete_at: i64,  // > 0 means archived
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dm_channel_name_format() {
        let user1 = "abc123";
        let user2 = "xyz789";

        let (id1, id2) = if user1 < user2 { (user1, user2) } else { (user2, user1) };
        let name = format!("__{id1}__{id2}__");

        assert_eq!(name, "__abc123__xyz789__");
    }
}
