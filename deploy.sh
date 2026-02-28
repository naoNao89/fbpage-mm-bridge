#!/bin/bash
set -e

# Deployment script for fbpage-mm-bridge
# This script can be run on the target server

DEPLOY_DIR="${DEPLOY_DIR:-/opt/fbpage-mm-bridge}"
SERVICE_NAME="fbpage-mm-bridge"

echo "🚀 Deploying $SERVICE_NAME to $DEPLOY_DIR"

# Create deployment directory
mkdir -p "$DEPLOY_DIR"
cd "$DEPLOY_DIR"

# Check if docker-compose.yml exists
if [ ! -f "docker-compose.yml" ]; then
    echo "❌ docker-compose.yml not found in $DEPLOY_DIR"
    exit 1
fi

# Check if .env exists
if [ ! -f ".env" ]; then
    echo "⚠️  .env file not found. Copying from .env.example..."
    if [ -f ".env.example" ]; then
        cp .env.example .env
        echo "⚠️  Please update .env with your configuration values"
        exit 1
    else
        echo "❌ .env.example not found either"
        exit 1
    fi
fi

# Pull latest images
echo "📥 Pulling latest Docker images..."
docker compose pull

# Run database migrations and restart
echo "🔄 Restarting services..."
docker compose up -d --force-recreate

# Show status
echo "✅ Deployment complete!"
echo ""
echo "Service status:"
docker compose ps

echo ""
echo "Logs: docker compose logs -f"
echo "Stop: docker compose down"
