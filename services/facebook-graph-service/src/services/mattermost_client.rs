//! Mattermost REST API client

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use reqwest::Client;
use serde::Deserialize;
use sqlx::PgPool;

/// Lightweight representations for API responses
#[derive(Debug, Deserialize)]
struct TeamResponse {
    id: String,
}

#[derive(Debug, Deserialize)]
struct ChannelResponse {
    id: String,
}

#[derive(Debug, Deserialize)]
struct PostResponse {
    id: String,
}

/// Mattermost REST API client
#[derive(Clone)]
pub struct MattermostClient {
    base_url: String,
    username: String,
    password: String,
    client: Client,
    token: Arc<Mutex<Option<String>>>,
    pool: Option<Arc<PgPool>>,

    channel_cache: Arc<Mutex<HashMap<String, String>>>, // conversation_id -> channel_id
    root_cache: Arc<Mutex<HashMap<String, String>>>,    // conversation_id -> root_post_id
}

impl MattermostClient {
    /// Create a new Mattermost client
    pub fn new(base_url: &str, username: &str, password: Option<&str>) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            username: username.to_string(),
            password: password.unwrap_or("").to_string(),
            client: Client::new(),
            token: Arc::new(Mutex::new(None)),
            pool: None,
            channel_cache: Arc::new(Mutex::new(HashMap::new())),
            root_cache: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Attach a database pool for persistent cache storage and load existing entries.
    pub async fn with_db_pool(mut self, pool: PgPool) -> Self {
        let pool = Arc::new(pool);
        match crate::db::load_mm_cache(&pool, "channel").await {
            Ok(channels) => {
                self.channel_cache
                    .lock()
                    .expect("channel_cache poisoned")
                    .extend(channels);
                tracing::info!(
                    "Loaded {} channel cache entries from database",
                    self.channel_cache.lock().unwrap().len()
                );
            }
            Err(e) => tracing::warn!("Failed to load channel cache from database: {e}"),
        }
        match crate::db::load_mm_cache(&pool, "root").await {
            Ok(roots) => {
                self.root_cache
                    .lock()
                    .expect("root_cache poisoned")
                    .extend(roots);
                tracing::info!(
                    "Loaded {} root cache entries from database",
                    self.root_cache.lock().unwrap().len()
                );
            }
            Err(e) => tracing::warn!("Failed to load root cache from database: {e}"),
        }
        self.pool = Some(pool);
        self
    }

    /// Login to Mattermost and cache the token
    pub async fn login(&self) -> Result<()> {
        let url = format!("{}/api/v4/users/login", self.base_url);
        let payload = serde_json::json!({
            "login_id": self.username,
            "password": self.password,
        });

        let resp = self
            .client
            .post(&url)
            .json(&payload)
            .send()
            .await
            .context("Failed to send login request to Mattermost")?;

        let status = resp.status();
        let token_from_header = resp
            .headers()
            .get("Token")
            .and_then(|h| h.to_str().ok())
            .map(String::from);
        let body_text = resp.text().await.unwrap_or_default();

        if !status.is_success() {
            return Err(anyhow::anyhow!(
                "Mattermost login failed with {status}: {body_text}"
            ));
        }

        let token = token_from_header
            .or_else(|| {
                serde_json::from_str::<serde_json::Value>(&body_text)
                    .ok()
                    .and_then(|j| j.get("token").and_then(|v| v.as_str()).map(String::from))
            })
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Mattermost login succeeded but no token in response header or body"
                )
            })?;

        let mut tok = self.token.lock().expect("token lock poisoned");
        *tok = Some(token);

        Ok(())
    }

    /// Ensure a valid token is available
    pub async fn ensure_token(&self) -> Result<()> {
        let needs_login = self.token.lock().expect("token lock poisoned").is_none();
        if needs_login {
            self.login().await?;
        }
        Ok(())
    }

    /// Get authorization header value, logging in if needed
    async fn get_auth_header(&self) -> Result<String> {
        let needs_login = self.token.lock().expect("token lock poisoned").is_none();
        if needs_login {
            self.login().await?;
        }
        let token = self.token.lock().expect("token lock poisoned").clone();
        token.ok_or_else(|| anyhow::anyhow!("No token after login"))
    }

    pub async fn get_team_id(&self) -> Result<String> {
        let auth = self.get_auth_header().await?;

        let url = format!("{}/api/v4/teams", self.base_url);
        let resp = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {auth}"))
            .send()
            .await
            .context("Failed to fetch teams from Mattermost")?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(
                "Mattermost get_teams failed with {status}: {body}"
            ));
        }

        let teams: Vec<TeamResponse> = resp
            .json()
            .await
            .context("Failed to parse Mattermost teams response")?;

        teams
            .first()
            .map(|t| t.id.clone())
            .ok_or_else(|| anyhow::anyhow!("No teams found in Mattermost"))
    }

    /// Get channel by name or create a new one, returning channel_id.
    ///
    /// Handles race conditions where another process may create the channel
    /// between our lookup and our create attempt. Also handles the case where
    /// the bot user hasn't been added to an existing channel by falling back
    /// to a team-level channel search.
    pub async fn get_or_create_channel(
        &self,
        team_id: &str,
        conversation_id: &str,
        display_name: &str,
    ) -> Result<String> {
        // Check cache first
        if let Some(cid) = self
            .channel_cache
            .lock()
            .expect("channel_cache poisoned")
            .get(conversation_id)
            .cloned()
        {
            return Ok(cid);
        }

        let name = conversation_id.to_lowercase();

        if let Some(cid) = self.fetch_channel_by_name(team_id, &name).await? {
            self.set_channel_cache(conversation_id, &cid);
            return Ok(cid);
        }

        let cid = self
            .create_channel_with_retry(team_id, &name, display_name)
            .await?;
        self.set_channel_cache(conversation_id, &cid);
        Ok(cid)
    }

    /// Fetch a channel by name within a team. Returns Ok(Some(id)) if found,
    /// Ok(None) if 404 (channel doesn't exist or bot not a member), or Err on
    /// unexpected failures.
    async fn fetch_channel_by_name(&self, team_id: &str, name: &str) -> Result<Option<String>> {
        let auth = self.get_auth_header().await?;
        let url = format!(
            "{}/api/v4/teams/{team_id}/channels/name/{name}",
            self.base_url
        );
        let resp = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {auth}"))
            .send()
            .await
            .context("Failed to query channel by name")?;

        if resp.status().is_success() {
            let ch: ChannelResponse = resp
                .json()
                .await
                .context("Failed to parse channel response")?;
            Ok(Some(ch.id))
        } else if resp.status().as_u16() == 404 {
            Ok(None)
        } else {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            Err(anyhow::anyhow!("Channel lookup failed {status}: {body}"))
        }
    }

    /// Search deleted channels in a team for a channel with the given name.
    /// Mattermost soft-deletes channels, preventing recreation with the same name.
    /// Returns the channel ID if found and successfully restored.
    async fn restore_deleted_channel(&self, team_id: &str, name: &str) -> Result<Option<String>> {
        let auth = self.get_auth_header().await?;
        let url = format!("{}/api/v4/teams/{team_id}/channels/deleted", self.base_url);
        let resp = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {auth}"))
            .send()
            .await
            .context("Failed to list deleted channels")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            tracing::warn!("Cannot list deleted channels: {status} {body}");
            return Ok(None);
        }

        let deleted: Vec<ChannelInfo> = resp
            .json()
            .await
            .context("Failed to parse deleted channels response")?;

        let found = deleted.into_iter().find(|c| c.name == name);
        let Some(ch) = found else {
            return Ok(None);
        };

        tracing::info!("Found deleted channel {name} (id={}), restoring", ch.id);
        let restore_url = format!("{}/api/v4/channels/{}/restore", self.base_url, ch.id);
        let restore_resp = self
            .client
            .post(&restore_url)
            .header("Authorization", format!("Bearer {auth}"))
            .send()
            .await
            .context("Failed to restore deleted channel")?;

        if restore_resp.status().is_success() {
            let restored: ChannelResponse = restore_resp
                .json()
                .await
                .context("Failed to parse restored channel")?;
            tracing::info!("Restored deleted channel {name} (id={})", restored.id);
            Ok(Some(restored.id))
        } else {
            let status = restore_resp.status();
            let body = restore_resp.text().await.unwrap_or_default();
            tracing::warn!("Failed to restore channel {name}: {status} {body}");
            Ok(None)
        }
    }

    /// Create a channel, handling the race condition where another process
    /// may have created it between our lookup and create attempt. Falls back
    /// to team-level search if the re-fetch by name fails (e.g. bot not yet
    /// a member of the newly-created channel).
    async fn create_channel_with_retry(
        &self,
        team_id: &str,
        name: &str,
        display_name: &str,
    ) -> Result<String> {
        let auth = self.get_auth_header().await?;

        let url_create = format!("{}/api/v4/channels", self.base_url);
        let payload = serde_json::json!({
            "team_id": team_id,
            "name": name,
            "display_name": display_name,
            "type": "O"
        });

        let resp = self
            .client
            .post(&url_create)
            .header("Authorization", format!("Bearer {auth}"))
            .json(&payload)
            .send()
            .await
            .context("Failed to create Mattermost channel")?;

        if resp.status().is_success() {
            let ch: ChannelResponse = resp
                .json()
                .await
                .context("Failed to parse channel create response")?;
            return Ok(ch.id);
        }

        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();

        if !body.contains("already exists") {
            return Err(anyhow::anyhow!(
                "Mattermost channel create failed {status}: {body}"
            ));
        }

        tracing::info!(
            "Channel {} already exists (race condition), resolving ID",
            name
        );

        if let Some(cid) = self.fetch_channel_by_name(team_id, name).await? {
            tracing::info!("Resolved existing channel {} via name lookup", name);
            return Ok(cid);
        }

        tracing::info!(
            "Cannot see channel {} by name (bot may not be a member), searching all team channels",
            name
        );
        let channels = self.list_channels_by_prefix(team_id, name).await?;
        let found = channels.into_iter().find(|c| c.name == name);

        if let Some(ch) = found {
            tracing::info!(
                "Resolved existing channel {} via team channel listing",
                name
            );
            return Ok(ch.id);
        }

        if let Some(cid) = self.restore_deleted_channel(team_id, name).await? {
            tracing::info!("Restored deleted channel {} and got ID {}", name, cid);
            return Ok(cid);
        }

        tracing::warn!(
            "Channel {} exists but cannot be resolved — attempting fresh login and retry",
            name
        );
        self.login().await?;

        if let Some(cid) = self.fetch_channel_by_name(team_id, name).await? {
            tracing::info!("Resolved existing channel {} after re-login", name);
            return Ok(cid);
        }

        Err(anyhow::anyhow!(
            "Channel '{name}' exists but could not be resolved (race condition + bot may lack membership). \
             Original create error: {status} {body}"
        ))
    }

    /// Post a message to a channel. Returns the new post_id.
    ///
    /// Skips posting if the channel already has a recent post with the same
    /// message text (handles webhook+poller race conditions where both
    /// paths process the same Facebook message).
    pub async fn post_message(
        &self,
        channel_id: &str,
        message: &str,
        root_id: Option<&str>,
        create_at: Option<i64>,
    ) -> Result<String> {
        if message.trim().is_empty() {
            return Err(anyhow::anyhow!("Skipping empty message post"));
        }

        if let Some(existing_id) = self
            .find_duplicate_post(channel_id, message, create_at)
            .await?
        {
            tracing::info!(
                "Skipping duplicate post in channel {channel_id}: message already exists as {existing_id}"
            );
            return Err(anyhow::anyhow!("Duplicate post skipped: {existing_id}"));
        }

        let auth = self.get_auth_header().await?;

        let url = format!("{}/api/v4/posts", self.base_url);
        let mut payload = serde_json::json!({
            "channel_id": channel_id,
            "message": message,
        });
        if let Some(rid) = root_id {
            payload.as_object_mut().unwrap().insert(
                "root_id".to_string(),
                serde_json::Value::String(rid.to_string()),
            );
        }
        if let Some(ts) = create_at {
            payload.as_object_mut().unwrap().insert(
                "create_at".to_string(),
                serde_json::Value::Number(ts.into()),
            );
        }

        let resp = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {auth}"))
            .json(&payload)
            .send()
            .await
            .context("Failed to send post to Mattermost")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("Mattermost post failed {status}: {body}"));
        }

        let post: PostResponse = resp.json().await.context("Failed to parse post response")?;
        Ok(post.id)
    }

    async fn find_duplicate_post(
        &self,
        channel_id: &str,
        message: &str,
        create_at: Option<i64>,
    ) -> Result<Option<String>> {
        let auth = self.get_auth_header().await?;
        let url = format!(
            "{}/api/v4/channels/{channel_id}/posts?per_page=10",
            self.base_url
        );

        let resp = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {auth}"))
            .send()
            .await
            .context("Failed to check for duplicate posts")?;

        if !resp.status().is_success() {
            return Ok(None);
        }

        let posts_data: serde_json::Value = resp
            .json()
            .await
            .context("Failed to parse posts response")?;

        let posts = match posts_data.get("posts").and_then(|p| p.as_object()) {
            Some(p) => p,
            None => return Ok(None),
        };

        for (_id, post) in posts {
            if post.get("message").and_then(|m| m.as_str()) == Some(message) {
                if let Some(ts) = create_at {
                    if post.get("create_at").and_then(|t| t.as_i64()) == Some(ts) {
                        let existing_id = post.get("id").and_then(|i| i.as_str()).unwrap_or("");
                        return Ok(Some(existing_id.to_string()));
                    }
                } else {
                    let existing_id = post.get("id").and_then(|i| i.as_str()).unwrap_or("");
                    return Ok(Some(existing_id.to_string()));
                }
            }
        }

        Ok(None)
    }

    /// Manually set root post id for a conversation
    pub fn set_root_id(&self, conversation_id: &str, post_id: &str) {
        self.root_cache
            .lock()
            .expect("root_cache poisoned")
            .insert(conversation_id.to_string(), post_id.to_string());
        self.persist_cache_entry("root", conversation_id, post_id);
    }

    /// Get the root post_id for a conversation if already posted as root
    pub async fn get_root_id(&self, conversation_id: &str) -> Result<Option<String>> {
        let guard = self.root_cache.lock().unwrap();
        Ok(guard.get(conversation_id).cloned())
    }

    /// Fetch all posts in a channel created after the given Unix millisecond timestamp.
    /// Returns posts sorted by creation time (oldest first).
    pub async fn get_posts_since(
        &self,
        channel_id: &str,
        since: i64,
    ) -> Result<Vec<MattermostPost>> {
        let auth = self.get_auth_header().await?;

        let url = format!(
            "{}/api/v4/channels/{channel_id}/posts?since={since}&per_page=60",
            self.base_url
        );

        let resp = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {auth}"))
            .send()
            .await
            .context("Failed to fetch posts from Mattermost")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(
                "Mattermost posts fetch failed {status}: {body}"
            ));
        }

        let posts_response: PostsResponse = resp
            .json()
            .await
            .context("Failed to parse Mattermost posts response")?;

        let mut posts: Vec<MattermostPost> = posts_response
            .order
            .iter()
            .filter_map(|id| posts_response.posts.get(id))
            .cloned()
            .collect();

        posts.sort_by_key(|p| p.create_at);
        Ok(posts)
    }

    /// List all channels for a team, filtered by name prefix.
    /// Used by the bot to discover channels that correspond to FB conversations.
    pub async fn list_channels_by_prefix(
        &self,
        team_id: &str,
        prefix: &str,
    ) -> Result<Vec<ChannelInfo>> {
        let auth = self.get_auth_header().await?;

        let url = format!(
            "{}/api/v4/teams/{team_id}/channels?per_page=200",
            self.base_url
        );

        let resp = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {auth}"))
            .send()
            .await
            .context("Failed to list Mattermost channels")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(
                "Mattermost channel list failed {status}: {body}"
            ));
        }

        let channels: Vec<ChannelInfo> = resp
            .json()
            .await
            .context("Failed to parse Mattermost channel list")?;

        Ok(channels
            .into_iter()
            .filter(|c| c.name.starts_with(prefix))
            .collect())
    }

    fn set_channel_cache(&self, conversation_id: &str, channel_id: &str) {
        self.channel_cache
            .lock()
            .expect("channel_cache poisoned")
            .insert(conversation_id.to_string(), channel_id.to_string());
        self.persist_cache_entry("channel", conversation_id, channel_id);
    }

    fn persist_cache_entry(&self, key_type: &str, conversation_id: &str, value: &str) {
        if let Some(pool) = &self.pool {
            let pool = Arc::clone(pool);
            let key_type = key_type.to_string();
            let conversation_id = conversation_id.to_string();
            let value = value.to_string();
            tokio::spawn(async move {
                if let Err(e) =
                    crate::db::upsert_mm_cache(pool.as_ref(), &key_type, &conversation_id, &value)
                        .await
                {
                    tracing::warn!(
                        "Failed to persist {key_type} cache entry for {conversation_id}: {e}"
                    );
                }
            });
        }
    }
}

// Data types for polling and channel listing

#[derive(Debug, Clone, Deserialize)]
pub struct MattermostPost {
    pub id: String,
    pub user_id: String,
    pub channel_id: String,
    pub message: String,
    pub root_id: String,
    #[serde(default)]
    pub create_at: i64,
}

#[derive(Debug, Deserialize)]
struct PostsResponse {
    pub order: Vec<String>,
    pub posts: HashMap<String, MattermostPost>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ChannelInfo {
    pub id: String,
    pub name: String,
    pub display_name: String,
    pub team_id: String,
}
