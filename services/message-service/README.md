# Message Service

A microservice for storing and managing messages from various platforms (Facebook, etc.) with Mattermost sync tracking.

## Overview

The Message Service is part of the FBPage-MM-Bridge system. It handles:
- Storing messages from Facebook Graph API
- Tracking Mattermost sync status
- Managing message metadata (conversation IDs, external IDs, etc.)

## API Endpoints

| Method | Endpoint | Description |
|--------|----------|-------------|
| GET | `/health` | Health check |
| POST | `/api/messages` | Store a new message |
| GET | `/api/messages/:id` | Get message by ID |
| GET | `/api/messages/customer/:customer_id` | Get messages by customer ID |
| GET | `/api/messages/conversation/:conversation_id` | Get messages by conversation |
| GET | `/api/messages/unsynced` | Get messages pending Mattermost sync |
| PUT | `/api/messages/:id/synced` | Mark message as synced |
| PUT | `/api/messages/:id/sync-failed` | Mark message sync as failed |

## Message Model

```rust
struct Message {
    id: Uuid,                      // Primary key
    customer_id: Uuid,             // FK to customers
    conversation_id: String,        // FB conversation ID
    platform: String,              // "facebook"
    direction: String,             // "incoming" or "outgoing"
    message_text: Option<String>,  // Message content
    external_id: Option<String>,   // FB message ID
    mattermost_channel: Option<String>,
    mattermost_synced_at: Option<DateTime<Utc>>,
    mattermost_sync_error: Option<String>,
    created_at: DateTime<Utc>,
}
```

## Configuration

Environment variables:

| Variable | Default | Description |
|----------|---------|-------------|
| `DATABASE_URL` | - | PostgreSQL connection URL (required) |
| `BIND_ADDRESS` | `0.0.0.0:3002` | Server bind address |
| `LOG_LEVEL` | `info` | Logging level |
| `CUSTOMER_SERVICE_URL` | `http://localhost:3001` | Customer Service URL |

## Quick Start

### Prerequisites

- Rust 1.75+
- PostgreSQL 14+
- Access to Customer Service (for customer validation)

### Development

1. Copy environment file:
   ```bash
   cp .env.example .env
   ```

2. Update `DATABASE_URL` in `.env`

3. Run the service:
   ```bash
   cargo run
   ```

### Docker

Build and run with Docker:

```bash
docker build -t message-service .
docker run -p 3002:3002 \
  -e DATABASE_URL=postgresql://user:pass@host:5432/db \
  -e CUSTOMER_SERVICE_URL=http://customer-service:3001 \
  message-service
```

## API Examples

### Create a Message

```bash
curl -X POST http://localhost:3002/api/messages \
  -H "Content-Type: application/json" \
  -d '{
    "customer_id": "uuid-here",
    "conversation_id": "t_122122340307156858",
    "platform": "facebook",
    "direction": "incoming",
    "message_text": "hello",
    "external_id": "msg_123456"
  }'
```

### Get Unsynced Messages

```bash
curl http://localhost:3002/api/messages/unsynced
```

### Mark as Synced

```bash
curl -X PUT http://localhost:3002/api/messages/{id}/synced \
  -H "Content-Type: application/json" \
  -d '{"mattermost_channel": "fb-customers-abc123"}'
```

## Architecture

```
┌─────────────────┐     ┌──────────────────┐     ┌─────────────────┐
│  Facebook API   │────▶│ Message Service  │────▶│   Mattermost    │
│  (Webhooks)     │     │                  │     │   Sync Worker   │
└─────────────────┘     └──────────────────┘     └─────────────────┘
                               │
                               ▼
                        ┌──────────────────┐
                        │ Customer Service │
                        │  (Validation)    │
                        └──────────────────┘
                               │
                               ▼
                        ┌──────────────────┐
                        │   PostgreSQL     │
                        └──────────────────┘
```

## License

MIT
