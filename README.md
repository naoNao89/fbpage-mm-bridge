# fbpage-mm-bridge

Bridge Facebook Page messages → Mattermost

## Quick Start

### Local Development

```bash
# Copy environment configuration
cp .env.example .env

# Edit .env with your values
# - MATTERMOST_WEBHOOK_URL
# - FACEBOOK_VERIFY_TOKEN
# - FACEBOOK_ACCESS_TOKEN

# Run with Docker Compose
docker compose up -d
```

### Production Deployment

1. **Set up GitHub Secrets** in your repository:
   - `SSH_PRIVATE_KEY` - SSH private key for server access
   - `SSH_HOST` - Server IP address
   - `SSH_USERNAME` - SSH username (e.g., `root`)
   - `DEPLOY_PATH` - Deployment directory (e.g., `/opt/fbpage-mm-bridge`)

2. **Generate SSH key pair** (if not exists):
   ```bash
   ssh-keygen -t ed25519 -C "github-actions@fbpage-mm-bridge"
   ```
   Add the public key to your server's `~/.ssh/authorized_keys`

3. **Push to main branch** - CI/CD will automatically:
   - Run tests
   - Build Docker image
   - Push to GitHub Container Registry
   - Deploy to your server

## Configuration

| Variable | Description | Default |
|----------|-------------|---------|
| `SERVER_HOST` | Server bind host | `0.0.0.0` |
| `SERVER_PORT` | Server port | `3000` |
| `DATABASE_URL` | PostgreSQL connection string | (required) |
| `MATTERMOST_WEBHOOK_URL` | Mattermost incoming webhook URL | (required) |
| `FACEBOOK_VERIFY_TOKEN` | Facebook webhook verification token | (required) |
| `FACEBOOK_ACCESS_TOKEN` | Facebook Page access token | (required) |

## Endpoints

- `GET /health` - Health check endpoint
- `POST /webhook/facebook` - Facebook webhook endpoint

## CI/CD Pipeline

```
Push to main → Test → Build Docker → Push to GHCR → Deploy via SSH
```

### Manual Deployment

```bash
# On target server
cd /opt/fbpage-mm-bridge
docker compose up -d
```

## License

MIT
# trigger
