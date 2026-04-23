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

#[derive(Debug, Deserialize)]
struct BotResponse {
    user_id: String,
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    token: String,
}

#[derive(Debug, Deserialize)]
struct FileUploadResponse {
    file_infos: Vec<FileInfoResponse>,
}

#[derive(Debug, Deserialize)]
struct FileInfoResponse {
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
    display_name_cache: Arc<Mutex<HashMap<String, String>>>,
    bot_user_cache: Arc<Mutex<HashMap<String, String>>>, // platform_user_id -> bot_user_id
    bot_token_cache: Arc<Mutex<HashMap<String, String>>>, // bot_user_id -> bot_token
    posted_ids: Arc<Mutex<std::collections::HashSet<String>>>, // external_id already posted to MM
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
            display_name_cache: Arc::new(Mutex::new(HashMap::new())),
            bot_user_cache: Arc::new(Mutex::new(HashMap::new())),
            bot_token_cache: Arc::new(Mutex::new(HashMap::new())),
            posted_ids: Arc::new(Mutex::new(std::collections::HashSet::new())),
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
        match crate::db::load_posted_message_ids(&pool).await {
            Ok(posted) => {
                let mut posted_ids = self.posted_ids.lock().expect("posted_ids poisoned");
                posted_ids.extend(posted);
                tracing::info!("Loaded {} posted message IDs from database", posted_ids.len());
            }
            Err(e) => tracing::warn!("Failed to load posted message IDs from database: {e}"),
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

    pub fn mark_posted(&self, external_id: &str) -> bool {
        self.posted_ids
            .lock()
            .expect("posted_ids poisoned")
            .insert(external_id.to_string())
    }

    pub async fn is_posted(&self, external_id: &str) -> bool {
        if self.posted_ids.lock().expect("posted_ids poisoned").contains(external_id) {
            return true;
        }
        if let Some(pool) = &self.pool {
            if let Ok(exists) = crate::db::is_message_posted(pool, external_id).await {
                if exists {
                    self.posted_ids.lock().expect("posted_ids poisoned").insert(external_id.to_string());
                    return true;
                }
            }
        }
        false
    }

    pub async fn maybe_update_display_name_by_conversation_id(
        &self,
        conversation_id: &str,
        display_name: &str,
    ) -> Result<(), anyhow::Error> {
        // Check cache first
        {
            let cache = self.display_name_cache.lock().expect("display_name_cache poisoned");
            if let Some(cached) = cache.get(conversation_id) {
                if cached == display_name {
                    return Ok(()); // Already set to same name
                }
            }
        }

        // Get team_id and channel
        let team_id = self.get_team_id().await?;
        let channel_id = self
            .get_or_create_channel(&team_id, conversation_id, display_name)
            .await?;

        self.update_channel_display_name(&channel_id, display_name).await?;

        // Update cache
        {
            let mut cache = self.display_name_cache.lock().expect("display_name_cache poisoned");
            cache.insert(conversation_id.to_string(), display_name.to_string());
        }

        Ok(())
    }

    pub async fn mark_posted_persistent(
        &self,
        external_id: &str,
        conversation_id: &str,
        mattermost_post_id: &str,
    ) -> bool {
        if let Some(pool) = &self.pool {
            match crate::db::mark_message_posted(
                pool,
                external_id,
                conversation_id,
                mattermost_post_id,
            )
            .await
            {
                Ok(true) => {
                    self.mark_posted(external_id);
                    tracing::debug!(
                        "Persisted mark_posted to DB: {} in conversation {}",
                        external_id,
                        conversation_id
                    );
                    return true;
                }
                Ok(false) => return false,
                Err(e) => tracing::warn!("Failed to persist mark_posted to DB: {e}"),
            }
        } else {
            tracing::warn!("MattermostClient pool is None, using in-memory only");
        }
        self.mark_posted(external_id);
        true
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
    pub async fn get_auth_header(&self) -> Result<String> {
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
        self.post_message_with_override(channel_id, message, root_id, create_at, None, None)
            .await
    }

    pub async fn post_message_with_override(
        &self,
        channel_id: &str,
        message: &str,
        root_id: Option<&str>,
        create_at: Option<i64>,
        override_username: Option<&str>,
        override_icon_url: Option<&str>,
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

        if override_username.is_some() || override_icon_url.is_some() {
            let mut props = serde_json::json!({});
            if let Some(uname) = override_username {
                props.as_object_mut().unwrap().insert(
                    "override_username".to_string(),
                    serde_json::Value::String(uname.to_string()),
                );
            }
            if let Some(icon) = override_icon_url {
                props.as_object_mut().unwrap().insert(
                    "override_icon".to_string(),
                    serde_json::Value::String(icon.to_string()),
                );
            }
            payload
                .as_object_mut()
                .unwrap()
                .insert("props".to_string(), props);
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

    /// Upload a file to Mattermost. Returns the file_id for attaching to a post.
    pub async fn upload_file(
        &self,
        channel_id: &str,
        file_data: bytes::Bytes,
        filename: &str,
        content_type: &str,
    ) -> Result<String> {
        let auth = self.get_auth_header().await?;
        let url = format!("{}/api/v4/files", self.base_url);

        let part = reqwest::multipart::Part::bytes(file_data.to_vec())
            .file_name(filename.to_string())
            .mime_str(content_type)
            .unwrap_or_else(|_| {
                reqwest::multipart::Part::bytes(file_data.to_vec()).file_name(filename.to_string())
            });

        let form = reqwest::multipart::Form::new()
            .text("channel_id", channel_id.to_string())
            .part("files", part);

        let resp = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {auth}"))
            .multipart(form)
            .send()
            .await
            .context("Failed to upload file to Mattermost")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(
                "Mattermost file upload failed {status}: {body}"
            ));
        }

        let upload_resp: FileUploadResponse = resp
            .json()
            .await
            .context("Failed to parse file upload response")?;

        upload_resp
            .file_infos
            .into_iter()
            .next()
            .map(|f| f.id)
            .ok_or_else(|| anyhow::anyhow!("No file_info in upload response"))
    }

    /// Upload a file to Mattermost using a bot token. Returns the file_id for attaching to a post.
    pub async fn upload_file_as_bot(
        &self,
        channel_id: &str,
        file_data: bytes::Bytes,
        filename: &str,
        content_type: &str,
        bot_token: &str,
    ) -> Result<String> {
        let url = format!("{}/api/v4/files", self.base_url);

        let part = reqwest::multipart::Part::bytes(file_data.to_vec())
            .file_name(filename.to_string())
            .mime_str(content_type)
            .unwrap_or_else(|_| {
                reqwest::multipart::Part::bytes(file_data.to_vec()).file_name(filename.to_string())
            });

        let form = reqwest::multipart::Form::new()
            .text("channel_id", channel_id.to_string())
            .part("files", part);

        let resp = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {bot_token}"))
            .multipart(form)
            .send()
            .await
            .context("Failed to upload file as bot to Mattermost")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("Bot file upload failed {status}: {body}"));
        }

        let upload_resp: FileUploadResponse = resp
            .json()
            .await
            .context("Failed to parse bot file upload response")?;

        upload_resp
            .file_infos
            .into_iter()
            .next()
            .map(|f| f.id)
            .ok_or_else(|| anyhow::anyhow!("No file_info in bot upload response"))
    }

    /// Post a message with file attachments to a channel. Returns the new post_id.
    pub async fn post_message_with_files(
        &self,
        channel_id: &str,
        message: &str,
        root_id: Option<&str>,
        create_at: Option<i64>,
        file_ids: &[String],
    ) -> Result<String> {
        if message.trim().is_empty() && file_ids.is_empty() {
            return Err(anyhow::anyhow!("Skipping empty message post with no files"));
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
        if !file_ids.is_empty() {
            payload.as_object_mut().unwrap().insert(
                "file_ids".to_string(),
                serde_json::Value::Array(
                    file_ids
                        .iter()
                        .map(|id| serde_json::Value::String(id.clone()))
                        .collect(),
                ),
            );
        }

        let resp = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {auth}"))
            .json(&payload)
            .send()
            .await
            .context("Failed to send post with files to Mattermost")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(
                "Mattermost post with files failed {status}: {body}"
            ));
        }

        let post: PostResponse = resp.json().await.context("Failed to parse post response")?;
        Ok(post.id)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn post_message_as_bot_with_files(
        &self,
        channel_id: &str,
        message: &str,
        root_id: Option<&str>,
        create_at: Option<i64>,
        bot_user_id: &str,
        bot_token: &str,
        file_ids: &[String],
    ) -> Result<String> {
        self.post_message_as_bot_with_files_and_override(
            channel_id,
            message,
            root_id,
            create_at,
            bot_user_id,
            bot_token,
            file_ids,
            None,
            None,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn post_message_as_bot_with_files_and_override(
        &self,
        channel_id: &str,
        message: &str,
        root_id: Option<&str>,
        create_at: Option<i64>,
        bot_user_id: &str,
        bot_token: &str,
        file_ids: &[String],
        override_username: Option<&str>,
        override_icon_url: Option<&str>,
    ) -> Result<String> {
        if message.trim().is_empty() && file_ids.is_empty() {
            return Err(anyhow::anyhow!(
                "Skipping empty bot message post with no files"
            ));
        }

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
        if !file_ids.is_empty() {
            payload.as_object_mut().unwrap().insert(
                "file_ids".to_string(),
                serde_json::Value::Array(
                    file_ids
                        .iter()
                        .map(|id| serde_json::Value::String(id.clone()))
                        .collect(),
                ),
            );
        }
        if override_username.is_some() || override_icon_url.is_some() {
            let mut props = serde_json::json!({});
            if let Some(uname) = override_username {
                props.as_object_mut().unwrap().insert(
                    "override_username".to_string(),
                    serde_json::Value::String(uname.to_string()),
                );
            }
            if let Some(icon) = override_icon_url {
                props.as_object_mut().unwrap().insert(
                    "override_icon".to_string(),
                    serde_json::Value::String(icon.to_string()),
                );
            }
            payload
                .as_object_mut()
                .unwrap()
                .insert("props".to_string(), props);
        }

        let resp = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {bot_token}"))
            .json(&payload)
            .send()
            .await
            .context("Failed to post message as bot with files")?;

        if resp.status().is_success() {
            let post: PostResponse = resp.json().await.context("Failed to parse post response")?;
            return Ok(post.id);
        }

        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();

        if status.as_u16() == 403 {
            let auth = self.get_auth_header().await?;
            let _ = self
                .add_user_to_channel(channel_id, bot_user_id, &auth)
                .await;

            let retry_resp = self
                .client
                .post(&url)
                .header("Authorization", format!("Bearer {bot_token}"))
                .json(&payload)
                .send()
                .await
                .context("Failed to retry post as bot with files")?;

            if retry_resp.status().is_success() {
                let post: PostResponse = retry_resp
                    .json()
                    .await
                    .context("Failed to parse post response")?;
                return Ok(post.id);
            }

            let retry_status = retry_resp.status();
            let retry_body = retry_resp.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(
                "Bot post with files failed after retry {retry_status}: {retry_body}"
            ));
        }

        Err(anyhow::anyhow!(
            "Bot post with files failed {status}: {body}"
        ))
    }

    async fn find_duplicate_post(
        &self,
        channel_id: &str,
        message: &str,
        create_at: Option<i64>,
    ) -> Result<Option<String>> {
        let auth = self.get_auth_header().await?;
        let url = format!(
            "{}/api/v4/channels/{channel_id}/posts?per_page=50",
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

    pub fn clear_root_id(&self, conversation_id: &str) {
        self.root_cache
            .lock()
            .expect("root_cache poisoned")
            .remove(conversation_id);
    }

    pub fn clear_root_id_db(&self, pool: &sqlx::PgPool, conversation_id: &str) {
        let cid = conversation_id.to_string();
        let pool = pool.clone();
        tokio::spawn(async move {
            if let Err(e) = sqlx::query(
                "DELETE FROM mattermost_cache WHERE key_type = 'root' AND conversation_id = $1",
            )
            .bind(&cid)
            .execute(&pool)
            .await
            {
                tracing::warn!("Failed to clear root_id from DB for {cid}: {e}");
            }
        });
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
    pub async fn get_all_t_channels(&self) -> Result<Vec<ChannelInfo>> {
        let team_id = self.get_team_id().await?;
        self.list_channels_by_prefix(&team_id, "t_").await
    }

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

    pub async fn get_or_create_customer_bot(
        &self,
        platform_user_id: &str,
        display_name: &str,
        channel_id: &str,
    ) -> Result<(String, String)> {
        let cached = {
            let uid = self
                .bot_user_cache
                .lock()
                .expect("bot_user_cache poisoned")
                .get(platform_user_id)
                .cloned();
            let token = uid.as_ref().and_then(|id| {
                self.bot_token_cache
                    .lock()
                    .expect("bot_token_cache poisoned")
                    .get(id)
                    .cloned()
            });
            uid.zip(token)
        };

        if let Some((bot_user_id, token)) = cached {
            let auth = self.get_auth_header().await.ok();
            if let Some(auth) = &auth {
                let _ = self
                    .add_user_to_channel(channel_id, &bot_user_id, auth)
                    .await;
            }
            return Ok((bot_user_id, token));
        }

        let auth = self.get_auth_header().await?;
        let team_id = self.get_team_id().await?;

        // Use PSID prefix to ensure unique usernames, preventing collisions when
        // multiple customers have the same display name
        let slug = self.generate_bot_username_with_psid(platform_user_id, display_name);
        let description = format!("FB Page customer PSID: {platform_user_id}");

        let create_url = format!("{}/api/v4/bots", self.base_url);
        let payload = serde_json::json!({
            "username": slug,
            "display_name": display_name,
            "description": description
        });

        let resp = self
            .client
            .post(&create_url)
            .header("Authorization", format!("Bearer {auth}"))
            .json(&payload)
            .send()
            .await
            .context("Failed to create customer bot")?;

        let bot_user_id = if resp.status().is_success() {
            let bot: BotResponse = resp.json().await.context("Failed to parse bot response")?;
            bot.user_id
        } else {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            if body.contains("already exists") || body.contains("must be unique") {
                tracing::info!("Bot username {slug} already exists, resolving existing bot");
                self.resolve_bot_user_by_username(&slug).await?
            } else {
                return Err(anyhow::anyhow!("Bot creation failed {status}: {body}"));
            }
        };

        if let Err(e) = self
            .enable_bot_and_add_to_team(&bot_user_id, &team_id, channel_id, &auth)
            .await
        {
            tracing::warn!("Bot enable/team/channel setup failed for {slug}: {e}, will retry channel add later");
        }

        let token_url = format!("{}/api/v4/users/{bot_user_id}/tokens", self.base_url);
        let token_payload = serde_json::json!({
            "description": "customer bot token"
        });
        let token_resp = self
            .client
            .post(&token_url)
            .header("Authorization", format!("Bearer {auth}"))
            .json(&token_payload)
            .send()
            .await
            .context("Failed to create bot token")?;

        let bot_token = if token_resp.status().is_success() {
            let tr: TokenResponse = token_resp
                .json()
                .await
                .context("Failed to parse token response")?;
            tr.token
        } else {
            let status = token_resp.status();
            let body = token_resp.text().await.unwrap_or_default();
            tracing::warn!("Failed to create bot token for {bot_user_id}: {status} {body}");
            return Err(anyhow::anyhow!(
                "Bot token creation failed {status}: {body}"
            ));
        };

        self.bot_user_cache
            .lock()
            .expect("bot_user_cache poisoned")
            .insert(platform_user_id.to_string(), bot_user_id.clone());
        self.bot_token_cache
            .lock()
            .expect("bot_token_cache poisoned")
            .insert(bot_user_id.clone(), bot_token.clone());

        tracing::info!(
            "Created customer bot {slug} (user_id={bot_user_id}) for channel {channel_id}"
        );

        let _ = self
            .add_user_to_channel(channel_id, &bot_user_id, &auth)
            .await;

        Ok((bot_user_id, bot_token))
    }

    pub fn generate_bot_username_from(display_name: &str) -> String {
        let ascii: String = display_name
            .to_lowercase()
            .chars()
            .map(|c| match c {
                'á' | 'à' | 'ả' | 'ã' | 'ạ' | 'ă' | 'ắ' | 'ằ' | 'ẳ' | 'ẵ' | 'ặ' | 'â' | 'ấ'
                | 'ầ' | 'ẩ' | 'ẫ' | 'ậ' => 'a',
                'đ' => 'd',
                'é' | 'è' | 'ẻ' | 'ẽ' | 'ẹ' | 'ê' | 'ế' | 'ề' | 'ể' | 'ễ' | 'ệ' => {
                    'e'
                }
                'í' | 'ì' | 'ỉ' | 'ĩ' | 'ị' => 'i',
                'ó' | 'ò' | 'ỏ' | 'õ' | 'ọ' | 'ô' | 'ố' | 'ồ' | 'ổ' | 'ỗ' | 'ộ' | 'ơ' | 'ớ'
                | 'ờ' | 'ở' | 'ỡ' | 'ợ' => 'o',
                'ú' | 'ù' | 'ủ' | 'ũ' | 'ụ' | 'ư' | 'ứ' | 'ừ' | 'ử' | 'ữ' | 'ự' => {
                    'u'
                }
                'ý' | 'ỳ' | 'ỷ' | 'ỹ' | 'ỵ' => 'y',
                _ => c,
            })
            .filter(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == ' ' || *c == '-')
            .collect::<String>();

        let slug: String = ascii
            .split(' ')
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("-");

        let slug: String = slug
            .chars()
            .filter(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == '-')
            .collect();

        let min_len = 3;
        let max_len = 22;
        if slug.len() < min_len {
            return format!("cust-{slug}");
        }
        if slug.len() > max_len {
            return slug[..max_len].to_string();
        }
        slug
    }

    pub fn generate_bot_username_with_psid(
        &self,
        platform_user_id: &str,
        display_name: &str,
    ) -> String {
        let name_slug = Self::generate_bot_username_from(display_name);
        let psid_prefix = if platform_user_id.len() >= 8 {
            platform_user_id[..8].to_lowercase()
        } else {
            platform_user_id.to_lowercase()
        };
        let combined = format!("{psid_prefix}-{name_slug}");
        let max_len = 22;
        if combined.len() > max_len {
            combined[..max_len].to_string()
        } else {
            combined
        }
    }

    async fn resolve_bot_user_by_username(&self, username: &str) -> Result<String> {
        let auth = self.get_auth_header().await?;
        let url = format!("{}/api/v4/users/username/{username}", self.base_url);
        let resp = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {auth}"))
            .send()
            .await
            .context("Failed to resolve bot user")?;

        if resp.status().is_success() {
            let user: serde_json::Value = resp.json().await.context("Failed to parse user")?;
            user.get("id")
                .and_then(|v| v.as_str())
                .map(String::from)
                .ok_or_else(|| anyhow::anyhow!("No user ID in response"))
        } else {
            Err(anyhow::anyhow!("Bot user not found: {username}"))
        }
    }

    async fn enable_bot_and_add_to_team(
        &self,
        bot_user_id: &str,
        team_id: &str,
        channel_id: &str,
        auth: &str,
    ) -> Result<()> {
        let enable_url = format!("{}/api/v4/bots/{bot_user_id}/enable", self.base_url);
        let enable_resp = self
            .client
            .post(&enable_url)
            .header("Authorization", format!("Bearer {auth}"))
            .send()
            .await;

        match enable_resp {
            Ok(resp) if resp.status().is_success() => {
                tracing::info!("Enabled bot {bot_user_id}");
            }
            Ok(resp) => {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                if !body.contains("already enabled") {
                    tracing::warn!("Failed to enable bot {bot_user_id}: {status} {body}");
                }
            }
            Err(e) => {
                tracing::warn!("Failed to enable bot {bot_user_id}: {e}");
            }
        }

        let team_member_url = format!("{}/api/v4/teams/{team_id}/members", self.base_url);
        let team_payload = serde_json::json!({
            "user_id": bot_user_id,
            "team_id": team_id,
        });
        let team_resp = self
            .client
            .post(&team_member_url)
            .header("Authorization", format!("Bearer {auth}"))
            .json(&team_payload)
            .send()
            .await;

        match team_resp {
            Ok(resp) if resp.status().is_success() => {
                tracing::info!("Added bot {bot_user_id} to team {team_id}");
            }
            Ok(resp) => {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                if !body.contains("already exists") && !body.contains("team_member") {
                    tracing::warn!(
                        "Failed to add bot {bot_user_id} to team {team_id}: {status} {body}"
                    );
                }
            }
            Err(e) => {
                tracing::warn!("Failed to add bot {bot_user_id} to team {team_id}: {e}");
            }
        }

        self.add_user_to_channel(channel_id, bot_user_id, auth)
            .await?;

        Ok(())
    }

    pub async fn post_message_as_bot(
        &self,
        channel_id: &str,
        message: &str,
        root_id: Option<&str>,
        create_at: Option<i64>,
        bot_user_id: &str,
        bot_token: &str,
    ) -> Result<String> {
        self.post_message_as_bot_with_override(
            channel_id,
            message,
            root_id,
            create_at,
            bot_user_id,
            bot_token,
            None,
            None,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn post_message_as_bot_with_override(
        &self,
        channel_id: &str,
        message: &str,
        root_id: Option<&str>,
        create_at: Option<i64>,
        bot_user_id: &str,
        bot_token: &str,
        override_username: Option<&str>,
        override_icon_url: Option<&str>,
    ) -> Result<String> {
        if message.trim().is_empty() {
            return Err(anyhow::anyhow!("Skipping empty message post"));
        }

        if let Some(existing_id) = self
            .find_duplicate_post(channel_id, message, create_at)
            .await?
        {
            tracing::info!(
                "Skipping duplicate bot post in channel {channel_id}: message already exists as {existing_id}"
            );
            return Err(anyhow::anyhow!("Duplicate post skipped: {existing_id}"));
        }

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
        if override_username.is_some() || override_icon_url.is_some() {
            let mut props = serde_json::json!({});
            if let Some(uname) = override_username {
                props.as_object_mut().unwrap().insert(
                    "override_username".to_string(),
                    serde_json::Value::String(uname.to_string()),
                );
            }
            if let Some(icon) = override_icon_url {
                props.as_object_mut().unwrap().insert(
                    "override_icon".to_string(),
                    serde_json::Value::String(icon.to_string()),
                );
            }
            payload
                .as_object_mut()
                .unwrap()
                .insert("props".to_string(), props);
        }

        let resp = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {bot_token}"))
            .json(&payload)
            .send()
            .await
            .context("Failed to post message as bot")?;

        if resp.status().is_success() {
            let post: PostResponse = resp.json().await.context("Failed to parse post response")?;
            return Ok(post.id);
        }

        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();

        if status.as_u16() == 403 {
            tracing::warn!("Bot {bot_user_id} got 403 posting to channel {channel_id}, re-adding to channel and retrying");

            let auth = self.get_auth_header().await?;
            let _ = self
                .add_user_to_channel(channel_id, bot_user_id, &auth)
                .await;

            let retry_resp = self
                .client
                .post(&url)
                .header("Authorization", format!("Bearer {bot_token}"))
                .json(&payload)
                .send()
                .await
                .context("Failed to retry post message as bot")?;

            if retry_resp.status().is_success() {
                let post: PostResponse = retry_resp
                    .json()
                    .await
                    .context("Failed to parse post response")?;
                tracing::info!("Bot {bot_user_id} retry post succeeded to channel {channel_id}");
                return Ok(post.id);
            }

            let retry_status = retry_resp.status();
            let retry_body = retry_resp.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(
                "Bot post failed after retry {retry_status}: {retry_body}"
            ));
        }

        if status.as_u16() == 400 && body.contains("root_id.app_error") {
            tracing::warn!(
                "Bot post got Invalid RootId, clearing root cache for channel {channel_id}"
            );
            self.clear_root_id(channel_id);
            return Err(anyhow::anyhow!("Invalid RootId cleared for {channel_id}"));
        }

        Err(anyhow::anyhow!("Bot post failed {status}: {body}"))
    }

    fn set_channel_cache(&self, conversation_id: &str, channel_id: &str) {
        self.channel_cache
            .lock()
            .expect("channel_cache poisoned")
            .insert(conversation_id.to_string(), channel_id.to_string());
        self.persist_cache_entry("channel", conversation_id, channel_id);
    }

    fn set_display_name_cache(&self, conversation_id: &str, display_name: &str) {
        self.display_name_cache
            .lock()
            .expect("display_name_cache poisoned")
            .insert(conversation_id.to_string(), display_name.to_string());
    }

    pub async fn maybe_update_display_name(
        &self,
        channel_id: &str,
        conversation_id: &str,
        desired_display_name: &str,
    ) -> Result<()> {
        if desired_display_name.is_empty() {
            return Ok(());
        }

        let cached = self
            .display_name_cache
            .lock()
            .expect("display_name_cache poisoned")
            .get(conversation_id)
            .cloned();

        if cached.as_deref() == Some(desired_display_name) {
            return Ok(());
        }

        if let Err(e) = self
            .update_channel_display_name(channel_id, desired_display_name)
            .await
        {
            tracing::warn!("Failed to update display_name for channel {channel_id}: {e}");
        } else {
            self.set_display_name_cache(conversation_id, desired_display_name);
        }

        Ok(())
    }

    pub async fn update_channel_display_name(
        &self,
        channel_id: &str,
        display_name: &str,
    ) -> Result<()> {
        let auth = self.get_auth_header().await?;
        let url = format!("{}/api/v4/channels/{channel_id}/patch", self.base_url);
        let payload = serde_json::json!({
            "display_name": display_name,
        });

        let resp = self
            .client
            .put(&url)
            .header("Authorization", format!("Bearer {auth}"))
            .json(&payload)
            .send()
            .await
            .context("Failed to update channel display_name")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(
                "Mattermost display_name update failed {status}: {body}"
            ));
        }

        tracing::info!("Updated display_name for channel {channel_id} to '{display_name}'");
        Ok(())
    }

    pub async fn ensure_bot_membership(&self, channel_id: &str) -> Result<()> {
        let auth = self.get_auth_header().await?;
        let bot_user_id = self.resolve_bot_user_id().await?;
        self.add_user_to_channel(channel_id, &bot_user_id, &auth)
            .await
    }

    async fn add_user_to_channel(&self, channel_id: &str, user_id: &str, auth: &str) -> Result<()> {
        let url = format!("{}/api/v4/channels/{channel_id}/members", self.base_url);
        let payload = serde_json::json!({
            "user_id": user_id,
        });

        let resp = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {auth}"))
            .json(&payload)
            .send()
            .await
            .context("Failed to add user to channel")?;

        if resp.status().is_success() {
            tracing::info!("Added user {user_id} to channel {channel_id}");
            self.cleanup_system_message(channel_id, auth).await;
        } else if resp.status().as_u16() == 400 {
            tracing::debug!("User {user_id} already a member of channel {channel_id}");
        } else {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            tracing::warn!("Could not add user {user_id} to channel {channel_id}: {status} {body}");
        }

        Ok(())
    }

    async fn cleanup_system_message(&self, channel_id: &str, auth: &str) {
        let url = format!(
            "{}/api/v4/channels/{channel_id}/posts?per_page=3",
            self.base_url
        );
        let resp = match self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {auth}"))
            .send()
            .await
        {
            Ok(r) => r,
            Err(_) => return,
        };

        let data: serde_json::Value = match resp.json().await {
            Ok(d) => d,
            Err(_) => return,
        };

        let posts = match data.get("posts").and_then(|p| p.as_object()) {
            Some(p) => p,
            None => return,
        };

        let admin_id = match self.resolve_bot_user_id().await.ok() {
            Some(id) => id,
            None => return,
        };

        for (_, post) in posts {
            let ptype = post.get("type").and_then(|t| t.as_str()).unwrap_or("");
            let uid = post.get("user_id").and_then(|u| u.as_str()).unwrap_or("");
            let pid = post.get("id").and_then(|i| i.as_str()).unwrap_or("");
            if (ptype.starts_with("system_") || ptype == "system_join_channel")
                && uid == admin_id
                && !pid.is_empty()
            {
                let del_url = format!("{}/api/v4/posts/{pid}", self.base_url);
                let _ = self
                    .client
                    .delete(&del_url)
                    .header("Authorization", format!("Bearer {auth}"))
                    .send()
                    .await;
                tracing::debug!("Cleaned up system message {pid} in channel {channel_id}");
                break;
            }
        }
    }

    async fn resolve_bot_user_id(&self) -> Result<String> {
        let auth = self.get_auth_header().await?;
        let url = format!("{}/api/v4/users/username/{}", self.base_url, self.username);

        let resp = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {auth}"))
            .send()
            .await
            .context("Failed to fetch bot user ID")?;

        if resp.status().is_success() {
            let user: serde_json::Value =
                resp.json().await.context("Failed to parse user response")?;
            user.get("id")
                .and_then(|v| v.as_str())
                .map(String::from)
                .ok_or_else(|| anyhow::anyhow!("No user ID in response"))
        } else {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            Err(anyhow::anyhow!(
                "Failed to resolve bot user ID: {status} {body}"
            ))
        }
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
