use std::{collections::HashMap, sync::Mutex, time::Duration};

use anyhow::{Context, Result};
use chrono::Utc;
use reqwest::Client;
use serde::Deserialize;
use tokio::time::sleep;
use tracing::{error, info, warn};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use uuid::Uuid;

const POLL_INTERVAL_MS: u64 = 2000;
const CHANNEL_NAME_PREFIX: &str = "t_";
const MAX_LOGIN_RETRIES: u32 = 10;
const LOGIN_RETRY_BASE_SECS: u64 = 2;

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();

    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "info".into()),
        ))
        .with(tracing_subscriber::fmt::layer())
        .init();

    let mattermost_url =
        std::env::var("MATTERMOST_URL").unwrap_or_else(|_| "http://mattermost:8065".to_string());
    let mattermost_username =
        std::env::var("MATTERMOST_USERNAME").unwrap_or_else(|_| "admin".to_string());
    let mattermost_password =
        std::env::var("MATTERMOST_PASSWORD").context("MATTERMOST_PASSWORD must be set")?;
    let facebook_page_access_token = std::env::var("FACEBOOK_PAGE_ACCESS_TOKEN")
        .context("FACEBOOK_PAGE_ACCESS_TOKEN must be set")?;
    let customer_service_url = std::env::var("CUSTOMER_SERVICE_URL")
        .unwrap_or_else(|_| "http://customer-service:3001".to_string());
    let message_service_url = std::env::var("MESSAGE_SERVICE_URL")
        .unwrap_or_else(|_| "http://message-service:3002".to_string());

    let http = Client::new();
    let mut mm = MmClient::new(&mattermost_url, &mattermost_username, &mattermost_password);

    let mut attempt = 0;
    loop {
        attempt += 1;
        match mm.login().await {
            Ok(()) => break,
            Err(e) if attempt <= MAX_LOGIN_RETRIES => {
                let delay = LOGIN_RETRY_BASE_SECS * 2u64.pow(attempt - 1);
                warn!("Mattermost login attempt {attempt}/{MAX_LOGIN_RETRIES} failed: {e:#}, retrying in {delay}s");
                sleep(Duration::from_secs(delay)).await;
            }
            Err(e) => {
                return Err(e).context("Mattermost login failed after all retries");
            }
        }
    }

    let team_id = mm
        .get_team_id()
        .await
        .context("Failed to get Mattermost team")?;
    info!("Mattermost team ID: {team_id}");

    info!("mm-bridge-bot started, polling every {POLL_INTERVAL_MS}ms");

    let mut last_poll_at: i64 = Utc::now().timestamp_millis();

    loop {
        match poll_and_respond(
            &http,
            &mm,
            &team_id,
            &facebook_page_access_token,
            &customer_service_url,
            &message_service_url,
            &mut last_poll_at,
        )
        .await
        {
            Ok(count) => {
                if count > 0 {
                    info!("Processed {count} new posts");
                }
            }
            Err(e) => {
                error!("Poll cycle failed: {e:#}");
            }
        }

        sleep(Duration::from_millis(POLL_INTERVAL_MS)).await;
    }
}

async fn poll_and_respond(
    http: &Client,
    mm: &MmClient,
    team_id: &str,
    fb_token: &str,
    customer_service_url: &str,
    message_service_url: &str,
    last_poll_at: &mut i64,
) -> Result<usize> {
    let channels = mm
        .list_channels_by_prefix(team_id, CHANNEL_NAME_PREFIX)
        .await?;

    let mut processed = 0;
    let mut newest_ts = *last_poll_at;

    for channel in channels {
        let posts = mm.get_posts_since(&channel.id, *last_poll_at).await?;

        let channel_newest = posts
            .iter()
            .map(|p| p.create_at)
            .max()
            .unwrap_or(*last_poll_at);
        if channel_newest > newest_ts {
            newest_ts = channel_newest;
        }

        for post in posts {
            let author_is_bot = if post.user_id == mm.bot_user_id {
                false
            } else {
                mm.is_bot_user(&post.user_id).await?
            };

            if !should_forward_post(&post, &mm.bot_user_id, author_is_bot) {
                continue;
            }

            let psid =
                match lookup_psid(&channel.name, message_service_url, customer_service_url).await {
                    Ok(p) => p,
                    Err(e) => {
                        warn!("Could not find PSID for conv {}: {e}", channel.name);
                        continue;
                    }
                };

            let fb_url =
                format!("https://graph.facebook.com/v24.0/me/messages?access_token={fb_token}");

            if !post.file_ids.is_empty() {
                for file_id in &post.file_ids {
                    if let Ok(file_info) = get_file_info(mm, file_id).await {
                        let image_url = match file_info.mime_type.as_deref() {
                            Some(mt) if mt.starts_with("image/") => file_info.url.clone(),
                            _ => continue,
                        };

                        let payload = serde_json::json!({
                            "recipient": {"id": psid},
                            "message": {
                                "attachment": {
                                    "type": "image",
                                    "payload": {"url": image_url}
                                }
                            }
                        });

                        let resp = http
                            .post(&fb_url)
                            .json(&payload)
                            .send()
                            .await
                            .context("Failed to send image to Facebook")?;

                        if resp.status().is_success() {
                            info!(
                                "Sent image '{}' (id={}) to FB user {psid} from MM file {file_id}",
                                file_info.name, file_info.id
                            );
                            processed += 1;
                        } else {
                            let err = resp.text().await.unwrap_or_default();
                            error!("Facebook image send API error: {err}");
                        }
                    }
                }
            }

            if !post.message.is_empty() {
                let payload = serde_json::json!({
                    "recipient": {"id": psid},
                    "message": {"text": post.message}
                });

                let resp = http
                    .post(&fb_url)
                    .json(&payload)
                    .send()
                    .await
                    .context("Failed to send to Facebook")?;

                if resp.status().is_success() {
                    info!(
                        "Replied to FB user {psid} from MM channel {} (post {})",
                        channel.name, post.id
                    );
                    processed += 1;
                } else {
                    let err = resp.text().await.unwrap_or_default();
                    error!("Facebook Send API error: {err}");
                }
            }
        }
    }

    if newest_ts > *last_poll_at {
        *last_poll_at = newest_ts;
    }

    Ok(processed)
}

async fn lookup_psid(
    conversation_id: &str,
    message_service_url: &str,
    customer_service_url: &str,
) -> Result<String> {
    #[derive(Deserialize)]
    struct MsgServiceResponse {
        customer_id: Uuid,
    }

    #[derive(Deserialize)]
    struct CustomerResponse {
        #[allow(dead_code)]
        id: String,
        platform_user_id: String,
    }

    let msg_url = format!(
        "{}/api/messages/conversation/{conversation_id}/customer",
        message_service_url.trim_end_matches('/')
    );

    let resp = reqwest::get(&msg_url)
        .await
        .with_context(|| format!("Failed to call message service for conv {conversation_id}"))?;

    if resp.status().as_u16() == 404 {
        anyhow::bail!("No messages found for conversation {conversation_id}");
    }

    if !resp.status().is_success() {
        anyhow::bail!(
            "Message service returned {} for conv {conversation_id}",
            resp.status()
        );
    }

    let msg_resp: MsgServiceResponse = resp
        .json()
        .await
        .context("Failed to parse message service response")?;

    let cust_url = format!(
        "{}/api/customers/{}",
        customer_service_url.trim_end_matches('/'),
        msg_resp.customer_id
    );

    let cust_resp = reqwest::get(&cust_url).await.with_context(|| {
        format!(
            "Failed to call customer service for {}",
            msg_resp.customer_id
        )
    })?;

    if !cust_resp.status().is_success() {
        anyhow::bail!(
            "Customer service returned {} for {}",
            cust_resp.status(),
            msg_resp.customer_id
        );
    }

    let customer: CustomerResponse = cust_resp
        .json()
        .await
        .context("Failed to parse customer response")?;

    Ok(customer.platform_user_id)
}

struct MmClient {
    base_url: String,
    username: String,
    password: String,
    http: Client,
    token: Option<String>,
    bot_user_id: String,
    bot_user_cache: Mutex<HashMap<String, bool>>,
}

impl MmClient {
    fn new(base_url: &str, username: &str, password: &str) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            username: username.to_string(),
            password: password.to_string(),
            http: Client::new(),
            token: None,
            bot_user_id: String::new(),
            bot_user_cache: Mutex::new(HashMap::new()),
        }
    }

    async fn login(&mut self) -> Result<()> {
        let url = format!("{}/api/v4/users/login", self.base_url);
        let payload = serde_json::json!({
            "login_id": self.username,
            "password": self.password,
        });

        let resp = self
            .http
            .post(&url)
            .json(&payload)
            .send()
            .await
            .context("Login request failed")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("MM login failed {status}: {body}");
        }

        self.token = resp
            .headers()
            .get("Token")
            .and_then(|h| h.to_str().ok())
            .map(|s| s.to_string());

        if let Ok(json) = resp.json::<serde_json::Value>().await {
            if let Some(id) = json.get("id").and_then(|v| v.as_str()) {
                self.bot_user_id = id.to_string();
            }
        }

        Ok(())
    }

    async fn get_team_id(&self) -> Result<String> {
        let token = self.token.as_ref().context("Not logged in")?;
        let url = format!("{}/api/v4/teams", self.base_url);

        let resp = self
            .http
            .get(&url)
            .header("Authorization", format!("Bearer {token}"))
            .send()
            .await
            .context("Failed to fetch teams")?;

        #[derive(Deserialize)]
        struct Team {
            id: String,
        }

        let teams: Vec<Team> = resp.json().await.context("Failed to parse teams")?;
        teams
            .first()
            .map(|t| t.id.clone())
            .ok_or_else(|| anyhow::anyhow!("No teams found"))
    }

    async fn list_channels_by_prefix(&self, team_id: &str, prefix: &str) -> Result<Vec<Channel>> {
        let token = self.token.as_ref().context("Not logged in")?;
        let url = format!(
            "{}/api/v4/teams/{team_id}/channels?per_page=200",
            self.base_url
        );

        let resp = self
            .http
            .get(&url)
            .header("Authorization", format!("Bearer {token}"))
            .send()
            .await
            .context("Failed to list channels")?;

        #[derive(Deserialize)]
        struct RawChannel {
            id: String,
            name: String,
            display_name: String,
            team_id: String,
        }

        let channels: Vec<RawChannel> = resp.json().await.context("Failed to parse channels")?;

        Ok(channels
            .into_iter()
            .filter(|c| c.name.starts_with(prefix))
            .map(|c| Channel {
                id: c.id,
                name: c.name,
                display_name: c.display_name,
                team_id: c.team_id,
            })
            .collect())
    }

    async fn get_posts_since(&self, channel_id: &str, since: i64) -> Result<Vec<Post>> {
        let token = self.token.as_ref().context("Not logged in")?;
        let url = format!(
            "{}/api/v4/channels/{channel_id}/posts?since={since}&per_page=60",
            self.base_url
        );

        let resp = self
            .http
            .get(&url)
            .header("Authorization", format!("Bearer {token}"))
            .send()
            .await
            .context("Failed to fetch posts")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("Posts fetch failed {status}: {body}");
        }

        #[derive(Deserialize)]
        struct PostsResp {
            order: Vec<String>,
            posts: std::collections::HashMap<String, Post>,
        }

        let data: PostsResp = resp.json().await.context("Failed to parse posts")?;

        Ok(ordered_posts_after_since(&data.order, &data.posts, since))
    }

    async fn is_bot_user(&self, user_id: &str) -> Result<bool> {
        if let Some(is_bot) = self
            .bot_user_cache
            .lock()
            .map_err(|_| anyhow::anyhow!("bot user cache poisoned"))?
            .get(user_id)
            .copied()
        {
            return Ok(is_bot);
        }

        let token = self.token.as_ref().context("Not logged in")?;
        let url = format!("{}/api/v4/users/{user_id}", self.base_url);

        let resp = self
            .http
            .get(&url)
            .header("Authorization", format!("Bearer {token}"))
            .send()
            .await
            .context("Failed to fetch Mattermost user")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("User fetch failed {status}: {body}");
        }

        #[derive(Deserialize)]
        struct UserResp {
            #[serde(default)]
            is_bot: bool,
        }

        let user: UserResp = resp.json().await.context("Failed to parse user")?;
        self.bot_user_cache
            .lock()
            .map_err(|_| anyhow::anyhow!("bot user cache poisoned"))?
            .insert(user_id.to_string(), user.is_bot);

        Ok(user.is_bot)
    }
}

fn should_forward_post(post: &Post, bot_user_id: &str, author_is_bot: bool) -> bool {
    post.user_id != bot_user_id
        && !author_is_bot
        && (!post.root_id.is_empty() || !post.message.is_empty() || !post.file_ids.is_empty())
}

fn ordered_posts_after_since(
    order: &[String],
    posts_by_id: &HashMap<String, Post>,
    since: i64,
) -> Vec<Post> {
    let mut posts: Vec<Post> = order
        .iter()
        .filter_map(|id| posts_by_id.get(id))
        .filter(|post| post.create_at > since)
        .cloned()
        .collect();

    posts.sort_by_key(|p| p.create_at);
    posts
}

#[derive(Debug, Clone, Deserialize)]
struct Post {
    id: String,
    user_id: String,
    #[allow(dead_code)]
    channel_id: String,
    message: String,
    root_id: String,
    #[serde(default)]
    create_at: i64,
    #[serde(default, deserialize_with = "deserialize_null_vec")]
    file_ids: Vec<String>,
}

fn deserialize_null_vec<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let opt: Option<Vec<String>> = Option::deserialize(deserializer)?;
    Ok(opt.unwrap_or_default())
}

#[derive(Debug, Clone)]
struct Channel {
    id: String,
    name: String,
    #[allow(dead_code)]
    display_name: String,
    #[allow(dead_code)]
    team_id: String,
}

#[derive(Debug, Deserialize)]
struct FileInfo {
    id: String,
    name: String,
    mime_type: Option<String>,
    url: String,
}

async fn get_file_info(mm: &MmClient, file_id: &str) -> Result<FileInfo> {
    let token = mm.token.as_ref().context("Not logged in")?;
    let url = format!("{}/api/v4/files/{file_id}", mm.base_url);

    let resp = mm
        .http
        .get(&url)
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .context("Failed to fetch file info")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("File info fetch failed {status}: {body}");
    }

    let info: FileInfo = resp.json().await.context("Failed to parse file info")?;
    Ok(info)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn post(id: &str, user_id: &str, create_at: i64, message: &str) -> Post {
        Post {
            id: id.to_string(),
            user_id: user_id.to_string(),
            channel_id: "channel".to_string(),
            message: message.to_string(),
            root_id: String::new(),
            create_at,
            file_ids: Vec::new(),
        }
    }

    #[test]
    fn customer_bot_post_is_not_forwarded_to_facebook() {
        let post = post(
            "fuyfqxs3f38udkf9amns3inh7r",
            "customer-bot",
            1777081506375,
            "Hé lu xốp",
        );

        assert!(!should_forward_post(&post, "admin-user", true));
    }

    #[test]
    fn own_admin_post_is_not_forwarded_to_facebook() {
        let post = post("admin-post", "admin-user", 1777081506376, "internal");

        assert!(!should_forward_post(&post, "admin-user", false));
    }

    #[test]
    fn human_non_empty_post_is_forwarded_to_facebook() {
        let post = post("human-post", "staff-user", 1777081506377, "reply");

        assert!(should_forward_post(&post, "admin-user", false));
    }

    #[test]
    fn post_at_since_boundary_is_not_reprocessed() {
        let boundary = post("boundary", "customer-bot", 1777081506375, "Hé lu xốp");
        let newer = post("newer", "staff-user", 1777081506376, "reply");
        let mut posts_by_id = HashMap::new();
        posts_by_id.insert(boundary.id.clone(), boundary);
        posts_by_id.insert(newer.id.clone(), newer);
        let order = vec!["newer".to_string(), "boundary".to_string()];

        let posts = ordered_posts_after_since(&order, &posts_by_id, 1777081506375);

        assert_eq!(posts.len(), 1);
        assert_eq!(posts[0].id, "newer");
    }
}
