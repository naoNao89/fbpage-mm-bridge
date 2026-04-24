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

    /// Archive a channel by setting deleteat timestamp
    ///
    /// In Mattermost, archived channels have deleteat > 0
    pub async fn archive_channel(&self, channelid: &str) -> Result<()> {
        let now = chrono::Utc::now().timestamp_millis();
        let affected = sqlx::query(
            r#"
            UPDATE channels
            SET deleteat = $1, updateat = $1
            WHERE id = $2 AND deleteat = 0
            "#,
        )
        .bind(now)
        .bind(channelid)
        .execute(&self.pool)
        .await
        .context("Failed to archive channel")?;

        if affected.rows_affected() == 0 {
            // Check if channel exists
            let exists: Option<(String, i64)> = sqlx::query_as(
                "SELECT id, deleteat FROM channels WHERE id = $1"
            )
            .bind(channelid)
            .fetch_optional(&self.pool)
            .await?;

            match exists {
                Some((_, deleteat)) if deleteat > 0 => {
                    tracing::info!("Channel {channelid} is already archived");
                }
                None => {
                    return Err(anyhow::anyhow!("Channel {channelid} not found"));
                }
                _ => {}
            }
        } else {
            tracing::info!("Archived channel {channelid}");
        }

        Ok(())
    }

    /// Unarchive a channel by clearing deleteat
    pub async fn unarchive_channel(&self, channelid: &str) -> Result<()> {
        let affected = sqlx::query(
            r#"
            UPDATE channels
            SET deleteat = 0, updateat = $1
            WHERE id = $2 AND deleteat > 0
            "#,
        )
        .bind(chrono::Utc::now().timestamp_millis())
        .bind(channelid)
        .execute(&self.pool)
        .await
        .context("Failed to unarchive channel")?;

        if affected.rows_affected() == 0 {
            return Err(anyhow::anyhow!(
                "Channel {channelid} not found or not archived"
            ));
        }

        tracing::info!("Unarchived channel {channelid}");
        Ok(())
    }

    /// Get channel info from database
    pub async fn get_channel(&self, channelid: &str) -> Result<Option<ChannelDbInfo>> {
        let channel = sqlx::query_as::<_, ChannelDbInfo>(
            "SELECT id, name, displayname, type, teamid, deleteat FROM channels WHERE id = $1",
        )
        .bind(channelid)
        .fetch_optional(&self.pool)
        .await?;

        Ok(channel)
    }

    /// Find a channel by name
    pub async fn find_channel_by_name(&self, name: &str) -> Result<Option<ChannelDbInfo>> {
        let channel = sqlx::query_as::<_, ChannelDbInfo>(
            "SELECT id, name, displayname, type, teamid, deleteat FROM channels WHERE name = $1",
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
        userid_1: &str,
        userid_2: &str,
    ) -> Result<String> {
        // Sort user IDs to ensure consistent channel name
        let (id1, id2) = if userid_1 < userid_2 {
            (userid_1, userid_2)
        } else {
            (userid_2, userid_1)
        };

        let channel_name = format!("__{id1}__{id2}__");

        // Try to find existing DM channel
        let existing: Option<ChannelDbInfo> = sqlx::query_as(
            "SELECT id, name, displayname, type, teamid, deleteat
             FROM channels WHERE name = $1 AND type = 'D'",
        )
        .bind(&channel_name)
        .fetch_optional(&self.pool)
        .await?;

        if let Some(channel) = existing {
            if channel.deleteat == 0 {
                tracing::debug!("Found existing DM channel: {}", channel.id);
                return Ok(channel.id);
            } else {
                // Unarchive if archived
                self.unarchive_channel(&channel.id).await?;
                return Ok(channel.id);
            }
        }

        // Create new DM channel
        let channelid = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().timestamp_millis();

        sqlx::query(
            r#"
            INSERT INTO channels (id, name, displayname, type, teamid, createat, updateat, deleteat, header, purpose)
            VALUES ($1, $2, 'Direct Message', 'D', NULL, $3, $3, 0, '', '')
            "#,
        )
        .bind(&channelid)
        .bind(&channel_name)
        .bind(now)
        .execute(&self.pool)
        .await
        .context("Failed to create DM channel")?;

        tracing::info!("Created DM channel {channelid} between {id1} and {id2}");
        Ok(channelid)
    }

    /// Add a user to a DM channel
    pub async fn add_user_to_dm_channel(
        &self,
        channelid: &str,
        userid: &str,
    ) -> Result<()> {
        let now = chrono::Utc::now().timestamp_millis();

        sqlx::query(
            r#"
            INSERT INTO channelmembers (channelid, userid, roles, lastviewedat, msgcount, mention_count)
            VALUES ($1, $2, 'channel_user', $3, 0, 0)
            ON CONFLICT (channelid, userid) DO NOTHING
            "#,
        )
        .bind(channelid)
        .bind(userid)
        .bind(now)
        .execute(&self.pool)
        .await
        .context("Failed to add user to DM channel")?;

        tracing::debug!("Added user {userid} to DM channel {channelid}");
        Ok(())
    }

    /// Send a message directly to a channel via database
    ///
    /// This bypasses all API restrictions and webhook limitations.
    pub async fn send_message(
        &self,
        channelid: &str,
        userid: &str,
        message: &str,
    ) -> Result<String> {
        let post_id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().timestamp_millis();

        sqlx::query(
            r#"
            INSERT INTO posts (id, channelid, userid, message, createat, updateat, deleteat, rootid, props)
            VALUES ($1, $2, $3, $4, $5, $5, 0, NULL, '{}')
            "#,
        )
        .bind(&post_id)
        .bind(channelid)
        .bind(userid)
        .bind(message)
        .bind(now)
        .execute(&self.pool)
        .await
        .context("Failed to insert post")?;

        sqlx::query(
            "UPDATE channels SET lastpostat = $1 WHERE id = $2",
        )
        .bind(now)
        .bind(channelid)
        .execute(&self.pool)
        .await
        .ok();

        tracing::info!("Sent message to channel {channelid}, post_id: {post_id}");
        Ok(post_id)
    }

    /// Send a DM from bot to user directly via database
    ///
    /// This is the primary use case - bots cannot send DMs via API in Team Edition.
    pub async fn send_bot_dm(
        &self,
        bot_userid: &str,
        target_userid: &str,
        message: &str,
    ) -> Result<String> {
        // Get or create DM channel
        let channelid = self.get_or_create_dm_channel(bot_userid, target_userid).await?;

        // Add bot to channel if not already member
        self.add_user_to_dm_channel(&channelid, bot_userid).await?;

        // Add target user to channel if not already member
        self.add_user_to_dm_channel(&channelid, target_userid).await?;

        // Send the message as the bot
        let post_id = self.send_message(&channelid, bot_userid, message).await?;

        tracing::info!(
            "Bot {bot_userid} sent DM to {target_userid}, post_id: {post_id}"
        );
        Ok(post_id)
    }

    /// Get user ID by username
    pub async fn get_userid_by_username(&self, username: &str) -> Result<Option<String>> {
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
            "SELECT id, name, displayname, type, teamid, deleteat
             FROM channels WHERE deleteat > 0 ORDER BY deleteat DESC",
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
                    "SELECT id, name, displayname, type, teamid, deleteat
                     FROM channels WHERE deleteat = 0 AND name LIKE $1 ORDER BY name",
                )
                .bind(format!("{p}%"))
                .fetch_all(&self.pool)
                .await?
            }
            None => {
                sqlx::query_as::<_, ChannelDbInfo>(
                    "SELECT id, name, displayname, type, teamid, deleteat
                     FROM channels WHERE deleteat = 0 ORDER BY name",
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
    pub displayname: String,
    pub r#type: String,  // O=public, P=private, D=direct
    pub teamid: Option<String>,
    pub deleteat: i64,  // > 0 means archived
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
