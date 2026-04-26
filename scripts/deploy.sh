#!/usr/bin/env bash
set -uo pipefail

STACK_DIR="${STACK_DIR:-/opt/mattermost}"
SECRETS_DIR="$STACK_DIR/secrets"
ADMIN_PW_FILE="$SECRETS_DIR/admin_password"

sudo mkdir -p "$STACK_DIR" "$SECRETS_DIR"
sudo chown -R "${USER:-$(whoami)}" "$STACK_DIR"

# ─── Git checkout ───────────────────────────────────────────────────────
if [ ! -d "$STACK_DIR/.git" ]; then
  if [ -d "$STACK_DIR" ] && [ -n "$(ls -A "$STACK_DIR" 2>/dev/null)" ]; then
    echo "Warning: $STACK_DIR exists but is not a git repo. Removing and re-cloning."
    rm -rf "$STACK_DIR"
  fi
  git clone --depth=1 "https://github.com/${REPO}" "$STACK_DIR"
else
  cd "$STACK_DIR"
  git fetch --depth=1 origin "$BRANCH"
  git reset --hard FETCH_HEAD
fi
cd "$STACK_DIR"

# ─── Admin password ─────────────────────────────────────────────────────
if [ -n "${MM_ADMIN_PASSWORD:-}" ]; then
  echo "Using MM_ADMIN_PASSWORD from GitHub Secrets."
  printf "%s" "$MM_ADMIN_PASSWORD" > "$ADMIN_PW_FILE"
  chmod 600 "$ADMIN_PW_FILE"
else
  if [ -f "$ADMIN_PW_FILE" ]; then
    export MM_ADMIN_PASSWORD="$(cat "$ADMIN_PW_FILE")"
    echo "Reusing existing generated admin password from $ADMIN_PW_FILE"
  else
    export MM_ADMIN_PASSWORD="$(openssl rand -base64 36 | tr -d '\n' | tr '+/' '-_' | cut -c1-24)"
    printf "%s" "$MM_ADMIN_PASSWORD" > "$ADMIN_PW_FILE"
    chmod 600 "$ADMIN_PW_FILE"
    echo "Generated new admin password and stored at $ADMIN_PW_FILE"
  fi
fi

# ─── Generate .env ──────────────────────────────────────────────────────
# Merge: env var > existing .env value > default
# Only write secrets/overrides; docker-compose.yml has defaults for the rest.
env_get() { [ -n "${1:-}" ] && echo "$1" || echo "$2"; }
env_merge() {
  local key="$1" default="$2"
  local env_val="${!key:-}"
  if [ -n "$env_val" ]; then printf '%s' "$env_val"; return; fi
  if [ -f .env ]; then
    local existing
    existing="$(grep -m1 "^${key}=" .env 2>/dev/null | cut -d= -f2-)"
    if [ -n "$existing" ]; then printf '%s' "$existing"; return; fi
  fi
  printf '%s' "$default"
}

PW=$(env_merge POSTGRES_PASSWORD postgres)
USER=$(env_merge POSTGRES_USER postgres)
MM_PW=$(env_merge MATTERMOST_PASSWORD "")
[ -z "$MM_PW" ] && MM_PW=$(env_merge MM_ADMIN_PASSWORD "")
image_tag_for() {
  if [ "${1:-}" = "success" ] && [ -n "${IMAGE_TAG:-}" ]; then
    printf '%s' "$IMAGE_TAG"
  else
    printf '%s' "latest"
  fi
}
CUSTOMER_SERVICE_IMAGE_TAG="$(image_tag_for "${BUILD_CUSTOMER:-}")"
MESSAGE_SERVICE_IMAGE_TAG="$(image_tag_for "${BUILD_MESSAGE:-}")"
FACEBOOK_GRAPH_SERVICE_IMAGE_TAG="$(image_tag_for "${BUILD_FACEBOOK:-}")"
MM_BRIDGE_BOT_IMAGE_TAG="$(image_tag_for "${BUILD_MM_BRIDGE_BOT:-}")"

cat > .env << ENVEOF
DATABASE_URL=postgres://${USER}:${PW}@customer-db:5432/customer_service
POSTGRES_USER=${USER}
POSTGRES_PASSWORD=${PW}
CUSTOMER_SERVICE_DB=customer_service
CUSTOMER_SERVICE_URL=http://customer-service:3001
MESSAGE_SERVICE_URL=http://message-service:3002
MATTERMOST_URL=http://mattermost:8065
MATTERMOST_USERNAME=$(env_merge MM_ADMIN_USERNAME admin)
MATTERMOST_PASSWORD=${MM_PW}
FACEBOOK_PAGE_ID=$(env_merge FACEBOOK_PAGE_ID "")
FACEBOOK_PAGE_ACCESS_TOKEN=$(env_merge FACEBOOK_PAGE_ACCESS_TOKEN "")
FACEBOOK_APP_ID=$(env_merge FACEBOOK_APP_ID "")
FACEBOOK_APP_SECRET=$(env_merge FACEBOOK_APP_SECRET "")
FACEBOOK_WEBHOOK_VERIFY_TOKEN=$(env_merge FACEBOOK_WEBHOOK_VERIFY_TOKEN "")
POLL_INTERVAL_SECS=$(env_merge POLL_INTERVAL_SECS 30)
MM_SITE_URL=$(env_merge MM_SITE_URL "")
MM_DOMAIN=$(env_merge MM_DOMAIN "")
MM_ADMIN_EMAIL=$(env_merge MM_ADMIN_EMAIL "")
MM_ADMIN_USERNAME=$(env_merge MM_ADMIN_USERNAME admin)
MM_ADMIN_PASSWORD=${MM_PW}
MM_BOT_USERNAME=$(env_merge MM_BOT_USERNAME "")
MM_BOT_DISPLAY_NAME=$(env_merge MM_BOT_DISPLAY_NAME "")
MM_BOT_DESCRIPTION=$(env_merge MM_BOT_DESCRIPTION "")
LOG_LEVEL=info
BIND_ADDRESS=0.0.0.0:3003
NGINX_HTTP_PORT=80
NGINX_HTTPS_PORT=443
NGINX_BIND_IP=0.0.0.0
MATTERMOST_PORT=8065
CUSTOMER_SERVICE_PORT=3001
MESSAGE_SERVICE_PORT=3002
FACEBOOK_GRAPH_SERVICE_PORT=3003
CUSTOMER_SERVICE_IMAGE_TAG=${CUSTOMER_SERVICE_IMAGE_TAG}
MESSAGE_SERVICE_IMAGE_TAG=${MESSAGE_SERVICE_IMAGE_TAG}
FACEBOOK_GRAPH_SERVICE_IMAGE_TAG=${FACEBOOK_GRAPH_SERVICE_IMAGE_TAG}
MM_BRIDGE_BOT_IMAGE_TAG=${MM_BRIDGE_BOT_IMAGE_TAG}
ENVEOF

echo ".env generated successfully"

# ─── Fix bind-mount paths ───────────────────────────────────────────────
for f in nginx/nginx.conf secrets/admin_password; do
  if [ -d "$f" ]; then
    echo "Fixing auto-created directory: $f -> removing"
    rm -rf "$f"
  fi
done
mkdir -p secrets nginx certs

# ─── SSL certificates ───────────────────────────────────────────────────
if [ -d "/etc/letsencrypt/live/${WEBHOOK_DOMAIN:-}" ]; then
  cp "/etc/letsencrypt/live/${WEBHOOK_DOMAIN}/fullchain.pem" certs/ 2>/dev/null || true
  cp "/etc/letsencrypt/live/${WEBHOOK_DOMAIN}/privkey.pem" certs/ 2>/dev/null || true
fi
if [ ! -f "certs/fullchain.pem" ] || [ ! -f "certs/privkey.pem" ]; then
  echo "WARNING: SSL certificates not found. Generating self-signed certs for nginx."
  openssl req -x509 -nodes -days 365 -newkey rsa:2048 \
    -keyout certs/privkey.pem -out certs/fullchain.pem \
    -subj "/CN=${WEBHOOK_DOMAIN:-localhost}" 2>/dev/null || true
fi

# ─── Docker network ─────────────────────────────────────────────────────
docker network inspect mattermost_fbpage-mm-network >/dev/null 2>&1 || \
  docker network create mattermost_fbpage-mm-network

# ─── Pull and deploy ────────────────────────────────────────────────────
export GHCR_OWNER="${GHCR_OWNER:-naonao89}"

echo "Pulling ALL service images from GHCR..."
if ! docker compose pull; then
  echo "ERROR: docker compose pull failed"
  exit 1
fi

docker compose down --remove-orphans --timeout 30 || echo "Warning: docker compose down failed (continuing)"

echo "Stopping all containers on VPS..."
docker stop $(docker ps -q) 2>/dev/null || true
docker rm -f $(docker ps -aq) 2>/dev/null || true

sleep 5

echo "Running docker compose up -d --force-recreate..."
if ! docker compose up -d --force-recreate; then
  echo "ERROR: docker compose up failed!"
  docker compose ps
  docker compose logs --tail=20
  exit 1
fi
echo "docker compose up -d completed successfully"

verify_image_tag() {
  local service="$1"
  local expected_tag="$2"
  if [ "$expected_tag" = "latest" ]; then
    return 0
  fi
  local container_id
  container_id="$(docker compose ps -q "$service" 2>/dev/null || true)"
  if [ -z "$container_id" ]; then
    echo "ERROR: $service container was not created"
    exit 1
  fi
  local running_image
  running_image="$(docker inspect --format='{{.Config.Image}}' "$container_id")"
  case "$running_image" in
    *":$expected_tag")
      echo "[PASS] $service running expected image tag $expected_tag"
      ;;
    *)
      echo "ERROR: $service is running $running_image, expected tag $expected_tag"
      exit 1
      ;;
  esac
}

verify_image_tag customer-service "$CUSTOMER_SERVICE_IMAGE_TAG"
verify_image_tag message-service "$MESSAGE_SERVICE_IMAGE_TAG"
verify_image_tag facebook-graph-service "$FACEBOOK_GRAPH_SERVICE_IMAGE_TAG"
verify_image_tag mm-bridge-bot "$MM_BRIDGE_BOT_IMAGE_TAG"

# ─── Wait for containers ────────────────────────────────────────────────
echo "Waiting for containers to be created..."
for i in $(seq 1 30); do
  RUNNING=$(docker compose ps --format '{{.Names}}' 2>/dev/null | wc -l)
  if [ "$RUNNING" -ge 10 ]; then
    echo "All $RUNNING containers created, waiting for startup..."
    break
  fi
  echo "Waiting... ($i/30) containers running: $RUNNING"
  sleep 2
done

echo "Waiting additional 60s for services to initialize..."
sleep 60

echo "=== DEBUG: service containers ==="
docker compose logs customer-service 2>&1 | tail -30
docker compose logs message-service 2>&1 | tail -30
docker compose logs facebook-graph-service 2>&1 | tail -30

echo "=== DEBUG: nginx container ==="
docker compose ps nginx
docker compose logs nginx 2>&1 | tail -20
ss -tlnp 'sport = :80' 2>/dev/null || echo "Port 80 not listening"
ss -tlnp 'sport = :443' 2>/dev/null || echo "Port 443 not listening"

MM_CONTAINER_ID="$(docker compose ps -q mattermost)"
if [ -n "$MM_CONTAINER_ID" ]; then
  echo "Admin password stored on server at: $ADMIN_PW_FILE"
else
  echo "WARNING: Could not find Mattermost container"
fi

# ─── Bootstrap Mattermost ───────────────────────────────────────────────
bash ./scripts/bootstrap-mm.sh

# ─── Post-deploy health checks ──────────────────────────────────────────
echo ""
echo "=== Post-Deploy Health Check ==="
echo ""

sleep 15

echo "Container status:"
docker compose ps

echo ""
echo "Service health checks:"

for service in customer-service message-service facebook-graph-service mm-bridge-bot; do
  for attempt in $(seq 1 12); do
    CONTAINER_ID=$(docker compose ps -q "$service" 2>/dev/null || true)
    if [ -n "$CONTAINER_ID" ]; then
      CONTAINER_STATUS=$(docker inspect --format='{{.State.Health.Status}}' "$CONTAINER_ID" 2>/dev/null || echo "no-healthcheck")
      if [ "$CONTAINER_STATUS" = "healthy" ] || [ "$CONTAINER_STATUS" = "no-healthcheck" ]; then
        echo "[PASS] $service: ready"
        break
      elif [ "$CONTAINER_STATUS" = "unhealthy" ]; then
        echo "[FAIL] $service: unhealthy"
        docker compose logs --tail=5 "$service"
        break
      fi
    fi
    if [ $attempt -eq 12 ]; then
      echo "[FAIL] $service: not healthy after 12 attempts"
    else
      echo "[WAIT] $service: attempt $attempt/12"
      sleep 5
    fi
  done
done

MM_CONTAINER_ID="$(docker compose ps -q mattermost)"
if [ -n "$MM_CONTAINER_ID" ]; then
  echo "[PASS] mattermost: running"
else
  echo "[WARN] mattermost: NOT running (continuing anyway)"
fi

echo ""
echo "=== Reloading nginx to refresh upstream DNS ==="
docker exec mattermost-nginx-1 nginx -s reload 2>/dev/null || echo "Warning: nginx reload failed"

echo "=== Deploy complete (health check failures are non-fatal) ==="
exit 0