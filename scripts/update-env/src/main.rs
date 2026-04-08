use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::Path;

fn read_existing_env(path: &Path) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let Ok(content) = fs::read_to_string(path) else {
        return map;
    };
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            map.insert(key.trim().to_string(), value.trim().to_string());
        }
    }
    map
}

fn val(env_key: &str, default: &str, existing: &HashMap<String, String>) -> String {
    match env::var(env_key) {
        Ok(v) if !v.is_empty() => v,
        _ => existing
            .get(env_key)
            .cloned()
            .unwrap_or_else(|| default.to_string()),
    }
}

fn main() {
    let existing = read_existing_env(Path::new(".env"));

    let pw = val("POSTGRES_PASSWORD", "postgres", &existing);
    let user = val("POSTGRES_USER", "postgres", &existing);
    let cust_db = val("CUSTOMER_SERVICE_DB", "customer_service", &existing);
    let mm_user = val("MM_ADMIN_USERNAME", "admin", &existing);
    let mm_pw = val("MATTERMOST_PASSWORD", "", &existing);
    let mm_pw = if mm_pw.is_empty() {
        val("MM_ADMIN_PASSWORD", "", &existing)
    } else {
        mm_pw
    };
    let fb_page_id = val("FACEBOOK_PAGE_ID", "", &existing);
    let fb_page_access_token = val("FACEBOOK_PAGE_ACCESS_TOKEN", "", &existing);
    let fb_app_id = val("FACEBOOK_APP_ID", "", &existing);
    let fb_app_secret = val("FACEBOOK_APP_SECRET", "", &existing);
    let fb_webhook_verify_token = val("FACEBOOK_WEBHOOK_VERIFY_TOKEN", "", &existing);
    let poll_interval_secs = val("POLL_INTERVAL_SECS", "30", &existing);
    let mm_site_url = val("MM_SITE_URL", "", &existing);
    let mm_domain = val("MM_DOMAIN", "", &existing);
    let mm_admin_email = val("MM_ADMIN_EMAIL", "", &existing);
    let mm_bot_username = val("MM_BOT_USERNAME", "", &existing);
    let mm_bot_display_name = val("MM_BOT_DISPLAY_NAME", "", &existing);
    let mm_bot_description = val("MM_BOT_DESCRIPTION", "", &existing);

    let out = format!(
        "\
DATABASE_URL=postgres://{user}:{pw}@customer-db:5432/{cust_db}
POSTGRES_USER={user}
POSTGRES_PASSWORD={pw}
CUSTOMER_SERVICE_DB={cust_db}
CUSTOMER_SERVICE_URL=http://customer-service:3001
MESSAGE_SERVICE_URL=http://message-service:3002
MATTERMOST_URL=http://mattermost:8065
MATTERMOST_USERNAME={mm_user}
MATTERMOST_PASSWORD={mm_pw}
FACEBOOK_PAGE_ID={fb_page_id}
FACEBOOK_PAGE_ACCESS_TOKEN={fb_page_access_token}
FACEBOOK_APP_ID={fb_app_id}
FACEBOOK_APP_SECRET={fb_app_secret}
FACEBOOK_WEBHOOK_VERIFY_TOKEN={fb_webhook_verify_token}
POLL_INTERVAL_SECS={poll_interval_secs}
MM_SITE_URL={mm_site_url}
MM_DOMAIN={mm_domain}
MM_ADMIN_EMAIL={mm_admin_email}
MM_ADMIN_USERNAME={mm_user}
MM_ADMIN_PASSWORD={mm_pw}
MM_BOT_USERNAME={mm_bot_username}
MM_BOT_DISPLAY_NAME={mm_bot_display_name}
MM_BOT_DESCRIPTION={mm_bot_description}
LOG_LEVEL=info
BIND_ADDRESS=0.0.0.0:3003
NGINX_HTTP_PORT=80
NGINX_HTTPS_PORT=443
NGINX_BIND_IP=0.0.0.0
MATTERMOST_PORT=8065
CUSTOMER_SERVICE_PORT=3001
MESSAGE_SERVICE_PORT=3002
FACEBOOK_GRAPH_SERVICE_PORT=3003
"
    );

    fs::write(".env", out).expect("failed to write .env");
    eprintln!(".env generated successfully");
}
