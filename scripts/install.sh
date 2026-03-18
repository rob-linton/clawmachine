#!/bin/bash
set -euo pipefail

INSTALL_DIR="${1:-$HOME/claw}"
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
if ! command -v docker &>/dev/null; then
  red "Missing: docker — install it and re-run."
  exit 1
fi
if ! docker compose version &>/dev/null && ! sudo docker compose version &>/dev/null; then
  red "Missing: docker compose v2"
  exit 1
fi

# Detect if we need sudo for docker (snap installs, etc.)
DOCKER="docker"
if ! docker info &>/dev/null 2>&1; then
  if sudo docker info &>/dev/null 2>&1; then
    DOCKER="sudo docker"
    yellow "  Using sudo for docker (snap or permissions)"
  else
    red "Cannot connect to Docker. Is the daemon running?"
    exit 1
  fi
fi
DC="$DOCKER compose"

# Install Node.js 20+ and npm if missing or too old (needed for Claude Code CLI)
NODE_VERSION=$(node --version 2>/dev/null | sed 's/v//' | cut -d. -f1 || echo "0")
if [ "$NODE_VERSION" -lt 18 ] 2>/dev/null; then
  yellow "  Node.js 18+ required for Claude Code CLI."
  yellow "  Install Node.js 20 (https://nodejs.org/) and re-run."
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

# --- Write Caddyfile (use spaces not tabs — heredoc-safe) ---
printf 'https://{$CLAW_HOST_IP}:443 {\n\ttls internal\n\treverse_proxy api:8080\n}\n\nhttp://{$CLAW_HOST_IP}:80 {\n\tredir https://{$CLAW_HOST_IP}{uri} permanent\n}\n' > Caddyfile
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
echo "  The worker needs Claude Code logged in on this machine."
echo "  The token is stored in ~/.claude.json (mounted into the container)."
echo ""

if ! command -v claude &>/dev/null; then
  yellow "  Installing Claude Code CLI..."
  npm install -g @anthropic-ai/claude-code 2>&1 | tail -1
fi

if [ ! -f "$HOME/.claude.json" ]; then
  echo "  No ~/.claude.json found. Launching Claude for login..."
  echo ""
  read -p "  Press Enter to start Claude login..."
  claude
  echo ""
elif claude -p "say ok" --output-format stream-json 2>&1 | grep -q "authentication_failed"; then
  echo "  Claude token expired. Launching login..."
  echo ""
  read -p "  Press Enter to start Claude login..."
  claude
  echo ""
else
  green "  Claude already authenticated"
fi

if [ -f "$HOME/.claude.json" ]; then
  green "  ~/.claude.json exists — worker will use it"
else
  red "  WARNING: ~/.claude.json not found. Worker will fail."
  red "  Run 'claude' manually to log in, then restart the worker."
fi

# --- Pull images and start ---
echo ""
echo "==========================================="
echo "  Pulling images and starting..."
echo "==========================================="
echo ""
ENV_FILE="$INSTALL_DIR/.env"
$DC --env-file "$ENV_FILE" pull 2>&1 | grep -v "^$" | tail -10
echo ""
$DC --env-file "$ENV_FILE" up -d 2>&1

# --- Wait for healthy ---
echo ""
echo "Waiting for API..."
for i in $(seq 1 30); do
  if $DC --env-file "$ENV_FILE" exec -T api curl -sf http://localhost:8080/api/v1/status >/dev/null 2>&1; then
    green "  API healthy!"
    break
  fi
  printf "."
  sleep 2
done

echo ""
$DC --env-file "$ENV_FILE" ps --format "table {{.Name}}\t{{.Status}}" 2>&1

# --- Extract CA cert ---
echo ""
$DC --env-file "$ENV_FILE" cp caddy:/data/caddy/pki/authorities/local/root.crt "$INSTALL_DIR/claw-ca.crt" 2>&1 | grep -v "Copying\|Copied" || true

echo ""
echo "==========================================="
green "  Install complete!"
echo ""
echo "  Dashboard:  https://$CLAW_HOST_IP"
echo "  Login:      $CLAW_ADMIN_USER / <your password>"
echo "  Install dir: $INSTALL_DIR"
echo ""
echo "  TLS CA cert: $INSTALL_DIR/claw-ca.crt"
echo "  Install this cert on team machines to avoid browser warnings."
echo ""
echo "  Commands (run from $INSTALL_DIR):"
echo "    $DC --env-file $ENV_FILE logs -f worker"
echo "    $DC --env-file $ENV_FILE restart worker"
echo "    $DC --env-file $ENV_FILE down"
echo "==========================================="
