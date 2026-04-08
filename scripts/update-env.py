#!/usr/bin/env python3
"""Generate .env from environment variables, preserving existing values when env vars are empty."""
import os


def _read_existing_env(path: str) -> dict[str, str]:
    """Read existing .env file into a dict (skipping comments and blank lines)."""
    existing = {}
    try:
        with open(path) as f:
            for line in f:
                line = line.strip()
                if not line or line.startswith("#"):
                    continue
                if "=" in line:
                    key, _, value = line.partition("=")
                    existing[key.strip()] = value.strip()
    except FileNotFoundError:
        pass
    return existing


def _val(env_key: str, default: str, existing: dict[str, str], cfg_key: str | None = None) -> str:
    """Return env var if set and non-empty, else existing .env value, else default.

    This prevents clobbering secrets that were set by a previous CI deploy
    when the script is run outside CI (e.g. manually on the server).
    """
    env_val = os.environ.get(env_key, "")
    if env_val:
        return env_val
    # Fall back to existing .env value
    key = cfg_key or env_key
    return existing.get(key, default)


def main() -> None:
    existing = _read_existing_env(".env")

    pw = _val("POSTGRES_PASSWORD", "postgres", existing)
    user = _val("POSTGRES_USER", "postgres", existing)
    mm_pw = _val("MATTERMOST_PASSWORD", "", existing) or _val("MM_ADMIN_PASSWORD", "", existing)

    env_content = f"""\
DATABASE_URL=postgres://{user}:{pw}@customer-db:5432/{_val("CUSTOMER_SERVICE_DB", "customer_service", existing)}
POSTGRES_USER={user}
POSTGRES_PASSWORD={pw}
CUSTOMER_SERVICE_DB={_val("CUSTOMER_SERVICE_DB", "customer_service", existing)}
CUSTOMER_SERVICE_URL=http://customer-service:3001
MESSAGE_SERVICE_URL=http://message-service:3002
MATTERMOST_URL=http://mattermost:8065
MATTERMOST_USERNAME={_val("MM_ADMIN_USERNAME", "admin", existing)}
MATTERMOST_PASSWORD={mm_pw}
FACEBOOK_PAGE_ID={_val("FACEBOOK_PAGE_ID", "", existing)}
FACEBOOK_PAGE_ACCESS_TOKEN={_val("FACEBOOK_PAGE_ACCESS_TOKEN", "", existing)}
FACEBOOK_APP_ID={_val("FACEBOOK_APP_ID", "", existing)}
FACEBOOK_APP_SECRET={_val("FACEBOOK_APP_SECRET", "", existing)}
FACEBOOK_WEBHOOK_VERIFY_TOKEN={_val("FACEBOOK_WEBHOOK_VERIFY_TOKEN", "", existing)}
POLL_INTERVAL_SECS={_val("POLL_INTERVAL_SECS", "30", existing)}
MM_SITE_URL={_val("MM_SITE_URL", "", existing)}
MM_DOMAIN={_val("MM_DOMAIN", "", existing)}
MM_ADMIN_EMAIL={_val("MM_ADMIN_EMAIL", "", existing)}
MM_ADMIN_USERNAME={_val("MM_ADMIN_USERNAME", "admin", existing)}
MM_ADMIN_PASSWORD={mm_pw}
MM_BOT_USERNAME={_val("MM_BOT_USERNAME", "", existing)}
MM_BOT_DISPLAY_NAME={_val("MM_BOT_DISPLAY_NAME", "", existing)}
MM_BOT_DESCRIPTION={_val("MM_BOT_DESCRIPTION", "", existing)}
LOG_LEVEL=info
BIND_ADDRESS=0.0.0.0:3003
NGINX_HTTP_PORT=80
NGINX_HTTPS_PORT=443
NGINX_BIND_IP=0.0.0.0
MATTERMOST_PORT=8065
CUSTOMER_SERVICE_PORT=3001
MESSAGE_SERVICE_PORT=3002
FACEBOOK_GRAPH_SERVICE_PORT=3003
"""

    with open(".env", "w") as f:
        f.write(env_content)

    print(".env generated successfully")


if __name__ == "__main__":
    main()
