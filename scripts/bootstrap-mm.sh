#!/usr/bin/env bash
# bootstrap-mm.sh — Provision Mattermost admin user + bot account
# Called by the CI/CD deploy workflow after `docker compose up -d`
# Expects the following environment variables to be set:
#   MM_SITE_URL        (e.g. https://chat.example.com)
#   MM_ADMIN_EMAIL
#   MM_ADMIN_USERNAME
#   MM_ADMIN_PASSWORD  (already written to container by deploy job)
#   MM_BOT_USERNAME
#   MM_BOT_DISPLAY_NAME
#   MM_BOT_DESCRIPTION

set -euo pipefail

MM_CONTAINER="${MM_CONTAINER:-mattermost}"
MMCTL="docker exec -i $MM_CONTAINER mmctl"

# Wait for Mattermost to be ready
echo "Waiting for Mattermost to be ready..."
for i in $(seq 1 30); do
  if docker exec "$MM_CONTAINER" mmctl system ping --local 2>/dev/null | grep -q "OK"; then
    echo "Mattermost is ready"
    break
  fi
  echo "   Attempt $i/30 — retrying in 5s..."
  sleep 5
done

echo "Provisioning admin user: $MM_ADMIN_USERNAME ..."
if ! $MMCTL user search "$MM_ADMIN_USERNAME" --local 2>/dev/null | grep -q "$MM_ADMIN_USERNAME"; then
  $MMCTL user create \
    --email "$MM_ADMIN_EMAIL" \
    --username "$MM_ADMIN_USERNAME" \
    --password "$MM_ADMIN_PASSWORD" \
    --system-admin \
    --local
  echo "Admin user created"
else
  echo "Admin user already exists"
fi

echo "Provisioning bot: $MM_BOT_USERNAME ..."
if ! $MMCTL bot list --local 2>/dev/null | grep -q "$MM_BOT_USERNAME"; then
  BOT_OWNER_ID=$($MMCTL user search "$MM_ADMIN_USERNAME" --local --json | \
    python3 -c "import sys,json; print(json.load(sys.stdin)[0]['id'])" 2>/dev/null || echo "")

  if [ -n "$BOT_OWNER_ID" ]; then
    $MMCTL bot create \
      --username "$MM_BOT_USERNAME" \
      --display-name "$MM_BOT_DISPLAY_NAME" \
      --description "$MM_BOT_DESCRIPTION" \
      --owner "$BOT_OWNER_ID" \
      --local
    echo "Bot account created"
  else
    echo "Could not determine bot owner ID - skipping bot creation"
  fi
else
  echo "Bot account already exists"
fi

echo ""
echo "Bootstrap complete"
