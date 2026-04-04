#!/bin/bash
set -euo pipefail

DOMAIN="${1:-webhook.bumpclean.com}"
WEBROOT="/var/www/_letsencrypt"

echo "Setting up SSL for $DOMAIN..."

if [ -d "/etc/letsencrypt/live/$DOMAIN" ]; then
  echo "SSL cert already exists for $DOMAIN."
  exit 0
fi

echo "Installing certbot..."
apt-get update -qq
apt-get install -y -qq certbot > /dev/null 2>&1

echo "Creating webroot for ACME challenge..."
mkdir -p "$WEBROOT"

echo "Creating temporary nginx config for ACME challenge..."
cat > /tmp/acme-challenge.conf << NGINX
server {
    listen 80;
    server_name $DOMAIN;

    location /.well-known/acme-challenge/ {
        root $WEBROOT;
    }

    location / {
        return 301 https://\$host\$request_uri;
    }
}
NGINX

sudo cp /tmp/acme-challenge.conf /etc/nginx/conf.d/acme-challenge.conf
sudo nginx -t && sudo systemctl reload nginx

echo "Requesting SSL certificate for $DOMAIN..."
certbot certonly \
  --webroot \
  --webroot-path "$WEBROOT" \
  --non-interactive \
  --agree-tos \
  --email "admin@$DOMAIN" \
  -d "$DOMAIN"

echo "SSL certificate obtained successfully."

echo "Setting up auto-renewal..."
EXISTING_CRON=$(crontab -l 2>/dev/null || true)
NEW_CRON=$(echo "$EXISTING_CRON" | grep -v "certbot renew" 2>/dev/null || true)
echo "$NEW_CRON" | crontab -
(crontab -l 2>/dev/null || echo ""; echo "0 3 * * * certbot renew --quiet --deploy-hook 'nginx -t && systemctl reload nginx'") | crontab -

echo "Done."
