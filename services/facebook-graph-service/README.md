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
