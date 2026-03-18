#!/bin/bash
set -euo pipefail

INSTALL_DIR="${1:-/opt/claw}"
REPO="ghcr.io/rob-linton/claudecodeclaw"

red()    { printf '\033[0;31m%s\033[0m\n' "$1"; }
green()  { printf '\033[0;32m%s\033[0m\n' "$1"; }
yellow() { printf '\033[0;33m%s\033[0m\n' "$1"; }

echo ""
echo "==========================================="
echo "  ClaudeCodeClaw — Install"
echo "==========================================="
echo ""

# --- Preflight ---
for cmd in docker npm; do
  if ! command -v $cmd &>/dev/null; then
    red "Missing: $cmd — install it and re-run."
    exit 1
  fi
done
if ! docker compose version &>/dev/null; then
  red "Missing: docker compose v2"
  exit 1
fi

# --- Create install directory ---
mkdir -p "$INSTALL_DIR"
cd "$INSTALL_DIR"
echo "Installing to: $INSTALL_DIR"
echo ""

# --- Gather config ---
DEFAULT_IP=$(hostname -I 2>/dev/null | awk '{print $1}' || echo "127.0.0.1")
read -p "Server IP address [$DEFAULT_IP]: " CLAW_HOST_IP
CLAW_HOST_IP="${CLAW_HOST_IP:-$DEFAULT_IP}"

read -p "Admin username [admin]: " CLAW_ADMIN_USER
CLAW_ADMIN_USER="${CLAW_ADMIN_USER:-admin}"

while true; do
  read -sp "Admin password: " CLAW_ADMIN_PASSWORD; echo
  if [ -n "$CLAW_ADMIN_PASSWORD" ]; then break; fi
  red "  Password cannot be empty."
done

CLAW_REDIS_PASSWORD=$(head -c 32 /dev/urandom | base64 | tr -d '/+=' | head -c 24)
echo ""

# --- Write .env ---
cat > .env <<ENVEOF
CLAW_HOST_IP=$CLAW_HOST_IP
CLAW_ADMIN_USER=$CLAW_ADMIN_USER
CLAW_ADMIN_PASSWORD=$CLAW_ADMIN_PASSWORD
CLAW_REDIS_PASSWORD=$CLAW_REDIS_PASSWORD
CLAW_REDIS_DB=0
CLAW_WORKER_CONCURRENCY=1
CLAW_WORKER_REPLICAS=1
CLAW_CRON_INTERVAL=30
RUST_LOG=info
ENVEOF
green "  .env written"

# --- Write Caddyfile ---
cat > Caddyfile <<'CADDYEOF'
https://{$CLAW_HOST_IP}:443 {
	tls internal
	reverse_proxy api:8080
}

http://{$CLAW_HOST_IP}:80 {
	redir https://{$CLAW_HOST_IP}{uri} permanent
}
CADDYEOF
green "  Caddyfile written"

# --- Write docker-compose.yml ---
cat > docker-compose.yml <<COMPOSEEOF
services:
  redis:
    image: redis:7-alpine
    restart: unless-stopped
    volumes:
      - redis_data:/data
    command: >
      redis-server
      --appendonly yes
      --appendfsync everysec
      --maxmemory 512mb
      --maxmemory-policy noeviction
      --requirepass \${CLAW_REDIS_PASSWORD:-changeme}
    healthcheck:
      test: ["CMD", "redis-cli", "-a", "\${CLAW_REDIS_PASSWORD:-changeme}", "ping"]
      interval: 5s
      timeout: 3s
      retries: 5
      start_period: 5s

  api:
    image: $REPO/api:latest
    restart: unless-stopped
    environment:
      CLAW_REDIS_URL: "redis://:\${CLAW_REDIS_PASSWORD:-changeme}@redis:6379/\${CLAW_REDIS_DB:-0}"
      CLAW_API_PORT: "8080"
      CLAW_STATIC_DIR: /app/static
      CLAW_API_TOKEN: "\${CLAW_API_TOKEN:-}"
      CLAW_ADMIN_USER: "\${CLAW_ADMIN_USER:-admin}"
      CLAW_ADMIN_PASSWORD: "\${CLAW_ADMIN_PASSWORD}"
      CLAW_CORS_ORIGIN: "https://\${CLAW_HOST_IP:-localhost}"
      RUST_LOG: "\${RUST_LOG:-info}"
    depends_on:
      redis:
        condition: service_healthy
    healthcheck:
      test: ["CMD", "curl", "-f", "http://localhost:8080/api/v1/status"]
      interval: 10s
      timeout: 5s
      retries: 3
      start_period: 10s

  worker:
    image: $REPO/worker:latest
    restart: unless-stopped
    environment:
      CLAW_REDIS_URL: "redis://:\${CLAW_REDIS_PASSWORD:-changeme}@redis:6379/\${CLAW_REDIS_DB:-0}"
      CLAW_WORKER_CONCURRENCY: "\${CLAW_WORKER_CONCURRENCY:-1}"
      RUST_LOG: "\${RUST_LOG:-info}"
      HOME: /home/claw
    depends_on:
      redis:
        condition: service_healthy
    volumes:
      - \${HOME}/.claude:/home/claw/.claude:ro
      - \${HOME}/.claude.json:/home/claw/.claude.json:ro
    deploy:
      replicas: \${CLAW_WORKER_REPLICAS:-1}

  scheduler:
    image: $REPO/scheduler:latest
    restart: unless-stopped
    environment:
      CLAW_REDIS_URL: "redis://:\${CLAW_REDIS_PASSWORD:-changeme}@redis:6379/\${CLAW_REDIS_DB:-0}"
      CLAW_CRON_CHECK_INTERVAL_SECS: "\${CLAW_CRON_INTERVAL:-30}"
      RUST_LOG: "\${RUST_LOG:-info}"
    depends_on:
      redis:
        condition: service_healthy

  caddy:
    image: caddy:2-alpine
    restart: unless-stopped
    ports:
      - "80:80"
      - "443:443"
    environment:
      CLAW_HOST_IP: "\${CLAW_HOST_IP:-localhost}"
    volumes:
      - ./Caddyfile:/etc/caddy/Caddyfile
      - caddy_data:/data
      - caddy_config:/config
    depends_on:
      api:
        condition: service_healthy

volumes:
  redis_data:
  caddy_data:
  caddy_config:
COMPOSEEOF
green "  docker-compose.yml written"

# --- Authenticate Claude Code ---
echo ""
echo "==========================================="
echo "  Step: Authenticate Claude Code"
echo "==========================================="
echo ""
echo "  The worker needs Claude Code logged in."
echo "  This will launch Claude interactively."
echo "  Follow the OAuth URL, then type /exit."
echo ""

if ! command -v claude &>/dev/null; then
  yellow "  Installing Claude Code CLI..."
  npm install -g @anthropic-ai/claude-code
fi

if [ ! -f "$HOME/.claude.json" ] || claude -p "say ok" --output-format stream-json 2>&1 | grep -q "authentication_failed"; then
  read -p "Press Enter to start Claude login..."
  claude
  echo ""
fi

if [ -f "$HOME/.claude.json" ]; then
  green "  Claude authenticated — ~/.claude.json exists"
else
  red "  WARNING: ~/.claude.json not found. Worker may fail."
  red "  Run 'claude' manually to log in, then restart the worker."
fi

# --- Pull images and start ---
echo ""
echo "==========================================="
echo "  Pulling images and starting..."
echo "==========================================="
echo ""
docker compose --env-file .env pull 2>&1 | grep -v "Pulling"
docker compose --env-file .env up -d 2>&1

# --- Wait for healthy ---
echo ""
echo "Waiting for API..."
for i in $(seq 1 30); do
  if docker compose --env-file .env exec -T api curl -sf http://localhost:8080/api/v1/status >/dev/null 2>&1; then
    break
  fi
  sleep 2
done

echo ""
docker compose --env-file .env ps --format "table {{.Name}}\t{{.Status}}"

# --- Extract CA cert ---
echo ""
docker compose --env-file .env cp caddy:/data/caddy/pki/authorities/local/root.crt ./claw-ca.crt 2>&1 | grep -v "Copying\|Copied" || true

echo ""
echo "==========================================="
green "  Install complete!"
echo ""
echo "  Dashboard:  https://$CLAW_HOST_IP"
echo "  Login:      $CLAW_ADMIN_USER / <your password>"
echo "  Install dir: $INSTALL_DIR"
echo ""
echo "  TLS CA cert: $INSTALL_DIR/claw-ca.crt"
echo "  Install this on each team member's machine"
echo "  to avoid browser security warnings."
echo ""
echo "  Useful commands:"
echo "    cd $INSTALL_DIR"
echo "    docker compose --env-file .env logs -f worker"
echo "    docker compose --env-file .env restart worker"
echo "    docker compose --env-file .env down"
echo "==========================================="
