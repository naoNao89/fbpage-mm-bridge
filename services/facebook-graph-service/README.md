# Facebook Graph Service

A microservice for fetching messages from Facebook Graph API and storing them via the Message Service.

## Overview

This service handles the import of Facebook conversations and messages by:
1. Fetching conversations from Facebook Graph API
2. Fetching messages for each conversation
3. Creating/getting customers via Customer Service
4. Storing messages via Message Service
5. Tracking direction (incoming/outgoing) based on sender ID vs page ID

## API Endpoints

- `GET /health` - Health check
- `POST /api/import/conversations` - Start import for all conversations
- `POST /api/import/conversation/:id` - Import single conversation by ID
- `GET /api/status` - Get import status

### Admin API (`/api/mm-admin/*`)

The Mattermost admin API is intended for internal operational tooling only.
Every endpoint requires `Authorization: Bearer $MM_ADMIN_API_TOKEN`.

| Endpoint | Description |
|----------|-------------|
| `GET /api/mm-admin/health` | Return bypass mode, DB availability, and Mattermost schema version |
| `DELETE /api/mm-admin/channels/:channel_id/posts` | Delete all posts from a Mattermost channel via the selected bypass strategy |
| `POST /api/mm-admin/channels/:channel_id/archive` | Archive a Mattermost channel |
| `POST /api/mm-admin/channels/:channel_id/unarchive` | Unarchive a Mattermost channel |
| `POST /api/mm-admin/dm` | Send a DM by writing directly to the Mattermost DB when bypass is enabled |

`MATTERMOST_BYPASS_MODE` controls mutating endpoints:

| Mode | Behavior |
|------|----------|
| `off` | Default. Mutating admin endpoints return `503`; existing public APIs continue to use REST API paths |
| `shadow` | Public reimport flows use the REST API path and write audit rows; direct DM/archive endpoints remain unavailable |
| `enabled` | DB-bypass paths are available when `MATTERMOST_DATABASE_URL` is configured |

Direct DB operations are audited in `mm_bypass_audit`. The DM endpoint accepts either an `Idempotency-Key` header or `idempotency_key` JSON field.

## Environment Variables

| Variable | Required | Description |
|----------|----------|-------------|
| `BIND_ADDRESS` | Yes | Server bind address (default: 0.0.0.0:3003) |
| `LOG_LEVEL` | No | Logging level (default: info) |
| `DATABASE_URL` | Yes | PostgreSQL connection URL |
| `FACEBOOK_PAGE_ID` | Yes | Facebook Page ID |
| `FACEBOOK_PAGE_ACCESS_TOKEN` | Yes | Facebook Page Access Token |
| `CUSTOMER_SERVICE_URL` | Yes | Customer Service URL |
| `MESSAGE_SERVICE_URL` | Yes | Message Service URL |
| `MATTERMOST_URL` | No | Mattermost REST API URL |
| `MATTERMOST_USERNAME` | No | Mattermost admin username |
| `MATTERMOST_PASSWORD` | No | Mattermost admin password |
| `MATTERMOST_DATABASE_URL` | No | Mattermost PostgreSQL URL for direct DB-bypass operations |
| `MATTERMOST_DATABASE_MAX_CONNECTIONS` | No | Max Mattermost DB pool connections (default: 5) |
| `MATTERMOST_BYPASS_MODE` | No | `off`, `shadow`, or `enabled` (default: off) |
| `MM_ADMIN_API_TOKEN` | No | Bearer token for `/api/mm-admin/*` |

## Building

```bash
# Development
cargo build

# Production
cargo build --release
```

## Running

```bash
# From source
cargo run

# From Docker
docker build -t facebook-graph-service .
docker run -p 3003:3003 --env-file .env facebook-graph-service
```

## Docker Compose

```yaml
services:
  facebook-graph-service:
    build: .
    ports:
      - "3003:3003"
    environment:
      - DATABASE_URL=postgres://postgres:postgres@db:5432/facebook_graph
      - FACEBOOK_PAGE_ID=${FACEBOOK_PAGE_ID}
      - FACEBOOK_PAGE_ACCESS_TOKEN=${FACEBOOK_PAGE_ACCESS_TOKEN}
      - CUSTOMER_SERVICE_URL=http://customer-service:3001
      - MESSAGE_SERVICE_URL=http://message-service:3002
    depends_on:
      - db
```

## API Usage

### Health Check

```bash
curl http://localhost:3003/health
```

### Start Full Import

```bash
curl -X POST http://localhost:3003/api/import/conversations
```

### Import Single Conversation

```bash
curl -X POST http://localhost:3003/api/import/conversation/t_122122340307156858
```

### Get Import Status

```bash
curl http://localhost:3003/api/status
```

## Architecture

```
┌─────────────────────┐
│ Facebook Graph API  │
└──────────┬──────────┘
           │
           ▼
┌─────────────────────┐
│ Facebook Graph      │
│ Service             │
│ - Fetch convos      │
│ - Fetch messages    │
│ - Rate limiting     │
└──────────┬──────────┘
           │
     ┌─────┴─────┐
     ▼           ▼
┌─────────┐ ┌───────────┐
│Customer │ │ Message   │
│Service  │ │ Service   │
└─────────┘ └───────────┘
```

## Rate Limiting

The service tracks Facebook API rate limits via:
- `X-App-Usage` response header
- `X-Business-Use-Case-Usage` header

Rate limit thresholds:
- Warning: 80% usage
- Critical: 95% usage (backoff applied)
