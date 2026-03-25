#!/bin/bash
set -e

# Deployment script for fbpage-mm-bridge microservices
# This script can be run on the target server

DEPLOY_DIR="${DEPLOY_DIR:-/opt/fbpage-mm-bridge}"
SERVICES=("customer-service" "message-service" "facebook-graph-service")
HEALTH_TIMEOUT=60
HEALTH_INTERVAL=5

echo "🚀 Deploying fbpage-mm-bridge microservices to $DEPLOY_DIR"

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

# Pull latest images and rebuild
echo "📥 Pulling latest Docker images..."
docker compose pull

# Build services (in case of local changes)
echo "🔨 Building Docker images..."
docker compose build

# Stop existing services gracefully
echo "🛑 Stopping existing services..."
docker compose down --remove-orphans

# Start services
echo "🔄 Starting services..."
docker compose up -d

# Wait for services to be healthy
echo "🏥 Waiting for services to be healthy..."
for service in "${SERVICES[@]}"; do
    echo "Checking $service..."
    count=0
    while [ $count -lt $((HEALTH_TIMEOUT / HEALTH_INTERVAL)) ]; do
        if docker compose exec -T "$service" curl -f http://localhost:3001/health 2>/dev/null || \
           docker compose exec -T "$service" curl -f http://localhost:3002/health 2>/dev/null || \
           docker compose exec -T "$service" curl -f http://localhost:3003/health 2>/dev/null; then
            echo "✅ $service is healthy"
            break
        fi
        count=$((count + 1))
        echo "   Waiting for $service... ($((count * HEALTH_INTERVAL))s)"
        sleep $HEALTH_INTERVAL
    done
    
    if [ $count -ge $((HEALTH_TIMEOUT / HEALTH_INTERVAL)) ]; then
        echo "❌ $service failed to become healthy within ${HEALTH_TIMEOUT}s"
        echo "📋 Service logs:"
        docker compose logs --tail=50 "$service"
        echo ""
        echo "Options:"
        echo "  1. Check logs: docker compose logs -f $service"
        echo "  2. Restart service: docker compose restart $service"
        echo "  3. Rollback: docker compose down && git log --oneline -1"
        exit 1
    fi
done

# Show status
echo ""
echo "✅ Deployment complete!"
echo ""
echo "Service status:"
docker compose ps

echo ""
echo "🌐 Service endpoints:"
echo "  Customer Service:      http://localhost:3001"
echo "  Message Service:      http://localhost:3002"
echo "  Facebook Graph:       http://localhost:3003"
echo ""
echo "📝 Common commands:"
echo "  View logs:     docker compose logs -f"
echo "  Stop:         docker compose down"
echo "  Restart:      docker compose restart"
echo "  Scale:        docker compose up -d --scale customer-service=2"
