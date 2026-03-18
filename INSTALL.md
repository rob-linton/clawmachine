# ClaudeCodeClaw — MVP POC Installation

Deploy on a dedicated Linux server with HTTPS and user authentication.

## Prerequisites

- Linux server (Ubuntu 22.04+ recommended)
- Docker 20.10+ with Docker Compose v2
- Node.js 18+ and npm (for Claude Code CLI)
- A browser to complete Claude Code OAuth login

## Step 1: Get the code onto the server

```bash
# Copy the project to the server (scp, rsync, git clone from private repo, etc.)
# Example with rsync from your dev machine:
rsync -az --exclude target --exclude .logs --exclude node_modules \
  /path/to/claudecodeclaw/ user@server:/opt/claudecodeclaw/

ssh user@server
cd /opt/claudecodeclaw
```

## Step 2: Configure

```bash
cp .env.example .env
nano .env
```

Set these values:

```
CLAW_HOST_IP=<server LAN IP>       # e.g. 192.168.1.50
CLAW_ADMIN_PASSWORD=<pick one>     # Dashboard login password
CLAW_REDIS_PASSWORD=<pick one>     # Internal Redis password
```

## Step 3: Authenticate Claude Code

The worker needs Claude Code logged in on the server. The OAuth token is stored in `~/.claude.json`.

```bash
./scripts/authenticate-worker.sh
```

This installs the Claude Code CLI (if needed) and launches it interactively. Follow the OAuth URL, complete login in your browser, then type `/exit`. The token is saved to `~/.claude.json` which the worker container mounts at runtime.

## Step 4: Build and start

```bash
docker compose -f docker/docker-compose.yml --env-file .env build
docker compose -f docker/docker-compose.yml --env-file .env up -d
```

First build takes ~10 minutes (Rust + Flutter compile inside Docker). Watch progress with:

```bash
docker compose -f docker/docker-compose.yml --env-file .env logs -f
```

Verify all 5 services are healthy:

```bash
docker compose -f docker/docker-compose.yml --env-file .env ps
```

## Step 5: Trust the TLS certificate

Caddy generates a self-signed CA for HTTPS. Extract and distribute to your team:

```bash
docker compose -f docker/docker-compose.yml --env-file .env cp \
  caddy:/data/caddy/pki/authorities/local/root.crt ./claw-ca.crt
```

Install on each client:
- **macOS**: Double-click `claw-ca.crt` → Keychain → mark "Always Trust"
- **Linux**: `sudo cp claw-ca.crt /usr/local/share/ca-certificates/ && sudo update-ca-certificates`
- **Windows**: Double-click → Install → "Trusted Root Certification Authorities"

## Step 6: Open the dashboard

Browse to `https://<server-ip>`. Sign in with username `admin` and the password from `.env`.

## Auto-start on boot

```bash
sudo cp scripts/claw.service /etc/systemd/system/
# Edit paths in the file if not installed to /opt/claudecodeclaw
sudo systemctl daemon-reload
sudo systemctl enable claw
sudo systemctl start claw
```

## Common operations

```bash
# View logs
docker compose -f docker/docker-compose.yml --env-file .env logs -f worker

# Restart after code update
./scripts/deploy.sh

# Re-authenticate Claude (e.g. token expired)
./scripts/authenticate-worker.sh
docker compose -f docker/docker-compose.yml --env-file .env restart worker

# Scale workers
CLAW_WORKER_REPLICAS=3 docker compose -f docker/docker-compose.yml --env-file .env up -d

# Stop everything
docker compose -f docker/docker-compose.yml --env-file .env down
```

## Architecture (what's running)

| Container | Purpose |
|-----------|---------|
| redis | Job queue + state store (password-protected) |
| api | REST API + serves Flutter dashboard |
| worker | Claims jobs, runs `claude -p`, stores results |
| scheduler | Cron jobs + file watcher |
| caddy | HTTPS reverse proxy (internal CA) |

The worker mounts `~/.claude.json` and `~/.claude/` from the host read-only for Claude Code authentication. No credentials are baked into Docker images.
