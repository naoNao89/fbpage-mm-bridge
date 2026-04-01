#!/usr/bin/env bash
set -e

COMPOSE_FILE="docker-compose.test.yml"
PROJECT_NAME="fbpage-mm-test"
TIMEOUT=300
CHECK_INTERVAL=5

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

log_info() {
    echo -e "${GREEN}[INFO]${NC} $1"
}

log_warn() {
    echo -e "${YELLOW}[WARN]${NC} $1"
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

cleanup() {
    log_info "Cleaning up test containers and volumes..."
    docker compose -f "$COMPOSE_FILE" -p "$PROJECT_NAME" down -v --remove-orphans 2>/dev/null || true
}

wait_for_healthy() {
    local service=$1
    local port=$2
    local elapsed=0

    log_info "Waiting for $service to be healthy (timeout: ${TIMEOUT}s)..."

    while [ $elapsed -lt $TIMEOUT ]; do
        if curl -sf "http://localhost:$port/health" > /dev/null 2>&1; then
            log_info "$service is healthy"
            return 0
        fi
        sleep $CHECK_INTERVAL
        elapsed=$((elapsed + CHECK_INTERVAL))
        echo -n "."
    done

    echo ""
    log_error "$service failed to become healthy within ${TIMEOUT}s"
    return 1
}

wait_for_database() {
    local db_container=$1
    local elapsed=0

    log_info "Waiting for $db_container to be ready..."

    while [ $elapsed -lt $TIMEOUT ]; do
        if docker exec "$db_container" pg_isready > /dev/null 2>&1; then
            log_info "$db_container is ready"
            return 0
        fi
        sleep $CHECK_INTERVAL
        elapsed=$((elapsed + CHECK_INTERVAL))
        echo -n "."
    done

    echo ""
    log_error "$db_container failed to become ready within ${TIMEOUT}s"
    return 1
}

trap cleanup EXIT

log_info "  Integration Test Infrastructure"

cleanup

log_info "Building and starting test services..."
docker compose -f "$COMPOSE_FILE" -p "$PROJECT_NAME" up --build -d

log_info "Waiting for databases to be ready..."
wait_for_database "fbpage-mm-test-customer-db"
wait_for_database "fbpage-mm-test-message-db"
wait_for_database "fbpage-mm-test-facebook-db"

log_info "Waiting for services to be healthy..."
wait_for_healthy "customer-service" "${TEST_CUSTOMER_SERVICE_PORT:-3101}"
wait_for_healthy "message-service" "${TEST_MESSAGE_SERVICE_PORT:-3102}"
wait_for_healthy "facebook-graph-service" "${TEST_FACEBOOK_GRAPH_SERVICE_PORT:-3103}"

log_info "All services are healthy. Running integration tests..."

docker compose -f "$COMPOSE_FILE" -p "$PROJECT_NAME" exec -T test-runner cargo test --release --workspace -- --test-threads=1

TEST_RESULT=$?

if [ $TEST_RESULT -eq 0 ]; then
    log_info "  Integration Tests PASSED"
else
    log_error "  Integration Tests FAILED (exit code: $TEST_RESULT)"
fi

exit $TEST_RESULT
