//! Mattermost REST API client

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use reqwest::Client;
use serde::Deserialize;

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

    // internal caches to avoid repeated lookups
    channel_cache: Arc<Mutex<HashMap<String, String>>>, // conversation_id -> channel_id
    root_cache: Arc<Mutex<HashMap<String, String>>>,    // conversation_id -> root_post_id
}

impl MattermostClient {
    /// Create a new Mattermost client
    pub fn new(base_url: &str, username: &str, password: &str) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            username: username.to_string(),
            password: password.to_string(),
            client: Client::new(),
            token: Arc::new(Mutex::new(None)),
            channel_cache: Arc::new(Mutex::new(HashMap::new())),
            root_cache: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Login to Mattermost and cache the token
    pub async fn login(&self) -> Result<()> {
        let url = format!("{}/api/v4/users/login", self.base_url);
        // Mattermost login uses login_id (username) and password
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

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(
                "Mattermost login failed with {status}: {body}"
            ));
        }

        // Token is returned in the header "Token"
        if let Some(token_header) = resp.headers().get("Token") {
            if let Ok(token_str) = token_header.to_str() {
                let mut tok = self.token.lock().expect("token lock poisoned");
                *tok = Some(token_str.to_string());
            }
        }

        // If not in header, try to read body as JSON with token field (fallback)
        if self.token.lock().unwrap().is_none() {
            if let Ok(json) = resp.json::<serde_json::Value>().await {
                if let Some(t) = json.get("token").and_then(|v| v.as_str()) {
                    let mut tok = self.token.lock().expect("token lock poisoned");
                    *tok = Some(t.to_string());
                }
            }
        }

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

    /// Retrieve the first team ID for the Mattermost instance
    pub async fn get_team_id(&self) -> Result<String> {
        self.ensure_token()
            .await
            .context("Failed to ensure token for get_team_id")?;

        let url = format!("{}/api/v4/teams", self.base_url);
        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .context("Failed to fetch teams from Mattermost")?;

        if !resp.status().is_success() {
            let status = resp.status();
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

    /// Get channel by name or create a new one, returning channel_id
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

        // Try to fetch by channel name (normalize name)
        let name = conversation_id.to_lowercase();
        let url_name = format!("{}/api/v4/channels/name/{name}", self.base_url);
        let resp = self
            .client
            .get(&url_name)
            .send()
            .await
            .context("Failed to query channel by name");
        if let Ok(r) = resp {
            if r.status().is_success() {
                let ch: ChannelResponse =
                    r.json().await.context("Failed to parse channel response")?;
                let cid = ch.id;
                self.channel_cache
                    .lock()
                    .unwrap()
                    .insert(conversation_id.to_string(), cid.clone());
                return Ok(cid);
            }
        }

        // Create channel if not found
        let team_id_clone = team_id.to_string();
        let url_create = format!("{}/api/v4/channels", self.base_url);
        let payload = serde_json::json!({
            "team_id": team_id_clone,
            "name": name,
            "display_name": display_name,
            "type": "O"
        });

        let resp = self
            .client
            .post(&url_create)
            .json(&payload)
            .send()
            .await
            .context("Failed to create Mattermost channel")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(
                "Mattermost channel create failed {status}: {body}"
            ));
        }

        let ch: ChannelResponse = resp
            .json()
            .await
            .context("Failed to parse channel create response")?;
        let cid = ch.id;
        self.channel_cache
            .lock()
            .unwrap()
            .insert(conversation_id.to_string(), cid.clone());
        Ok(cid)
    }

    /// Post a message to a channel. Returns the new post_id
    pub async fn post_message(
        &self,
        channel_id: &str,
        message: &str,
        root_id: Option<&str>,
    ) -> Result<String> {
        self.ensure_token()
            .await
            .context("Failed to ensure token for posting message")?;

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

        let resp = self
            .client
            .post(&url)
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
        // Cache the root_id if this is the first post (root)
        if root_id.is_none() {
            // Use conversation-channel key to store root post id. We don't have conversation_id here; rely on caller to set root via separate method if needed.
            // For safety, store under a composite key built outside (caller passes conversation_id logic via separate setter).
            // Here, we do not set a root_id automatically; caller will set via set_root_id if needed.
        }
        Ok(post.id)
    }

    /// Manually set root post id for a conversation
    pub fn set_root_id(&self, conversation_id: &str, post_id: &str) {
        self.root_cache
            .lock()
            .expect("root_cache poisoned")
            .insert(conversation_id.to_string(), post_id.to_string());
    }

    /// Get the root post_id for a conversation if already posted as root
    pub async fn get_root_id(&self, conversation_id: &str) -> Result<Option<String>> {
        // We store root_ids under the conversation_id key in root_cache
        let guard = self.root_cache.lock().unwrap();
        Ok(guard.get(conversation_id).cloned())
    }
}

// Re-export anyhow for internal error construction if needed
