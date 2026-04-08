#!/usr/bin/env python3
"""Generate .env from environment variables."""
import os

env_content = f"""DATABASE_URL=postgres://{os.environ.get('POSTGRES_USER', 'postgres')}:{os.environ.get('POSTGRES_PASSWORD', '')}@customer-db:5432/{os.environ.get('CUSTOMER_SERVICE_DB', 'customer_service')}
POSTGRES_USER={os.environ.get('POSTGRES_USER', 'postgres')}
POSTGRES_PASSWORD={os.environ.get('POSTGRES_PASSWORD', '')}
CUSTOMER_SERVICE_DB={os.environ.get('CUSTOMER_SERVICE_DB', 'customer_service')}
CUSTOMER_SERVICE_URL=http://customer-service:3001
MESSAGE_SERVICE_URL=http://message-service:3002
MATTERMOST_URL=http://mattermost:8065
MATTERMOST_USERNAME={os.environ.get('MM_ADMIN_USERNAME', 'admin')}
MATTERMOST_PASSWORD={os.environ.get('MATTERMOST_PASSWORD', '') or os.environ.get('MM_ADMIN_PASSWORD', '')}
FACEBOOK_PAGE_ID={os.environ.get('FACEBOOK_PAGE_ID', '')}
FACEBOOK_PAGE_ACCESS_TOKEN={os.environ.get('FACEBOOK_PAGE_ACCESS_TOKEN', '')}
FACEBOOK_APP_ID={os.environ.get('FACEBOOK_APP_ID', '')}
FACEBOOK_APP_SECRET={os.environ.get('FACEBOOK_APP_SECRET', '')}
FACEBOOK_WEBHOOK_VERIFY_TOKEN={os.environ.get('FACEBOOK_WEBHOOK_VERIFY_TOKEN', '')}
MM_SITE_URL={os.environ.get('MM_SITE_URL', '')}
MM_DOMAIN={os.environ.get('MM_DOMAIN', '')}
MM_ADMIN_EMAIL={os.environ.get('MM_ADMIN_EMAIL', '')}
MM_ADMIN_USERNAME={os.environ.get('MM_ADMIN_USERNAME', 'admin')}
MM_ADMIN_PASSWORD={os.environ.get('MM_ADMIN_PASSWORD', '')}
MM_BOT_USERNAME={os.environ.get('MM_BOT_USERNAME', '')}
MM_BOT_DISPLAY_NAME={os.environ.get('MM_BOT_DISPLAY_NAME', '')}
MM_BOT_DESCRIPTION={os.environ.get('MM_BOT_DESCRIPTION', '')}
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

with open('.env', 'w') as f:
    f.write(env_content)

print('.env generated successfully')
