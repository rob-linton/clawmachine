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

# Check Node.js (needed for Claude Code CLI install, but not if already installed)
if ! command -v claude &>/dev/null; then
  NODE_VERSION=$(node --version 2>/dev/null | sed 's/v//' | cut -d. -f1 || echo "0")
  if [ "$NODE_VERSION" -lt 18 ] 2>/dev/null; then
    yellow "  Node.js 18+ required to install Claude Code CLI."
    yellow "  Install Node.js 20 (https://nodejs.org/) and re-run."
    yellow "  Or install Claude Code separately: npm install -g @anthropic-ai/claude-code"
    exit 1
  fi
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

# --- Data directory ---
# Use home dir to avoid permission issues (no sudo needed)
CLAW_DATA_DIR="$HOME/.claw-data"
mkdir -p "$CLAW_DATA_DIR"
green "  Data directory: $CLAW_DATA_DIR"

# Migrate from named volume if it exists (upgrade scenario)
if $DOCKER volume inspect claw_data &>/dev/null 2>&1; then
  VOLUME_PATH=$($DOCKER volume inspect claw_data --format '{{ .Mountpoint }}' 2>/dev/null || true)
  if [ -n "$VOLUME_PATH" ] && [ -d "$VOLUME_PATH" ]; then
    yellow "  Found existing claw_data volume at $VOLUME_PATH"
    yellow "  Migrating data to $CLAW_DATA_DIR..."
    sudo cp -a "$VOLUME_PATH/." "$CLAW_DATA_DIR/" 2>/dev/null || true
    sudo chown -R "$(id -u):$(id -g)" "$CLAW_DATA_DIR" 2>/dev/null || true
    green "  Data migrated. Old volume can be removed with: $DOCKER volume rm claw_data"
  fi
fi

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
CLAUDE_HOME=$HOME/.claude
CLAUDE_JSON=$HOME/.claude.json
CLAW_DATA_DIR=$CLAW_DATA_DIR
CLAW_EXECUTION_BACKEND=docker
ENVEOF
green "  .env written"

# --- Write Caddyfile ---
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
    volumes:
      - /var/run/docker.sock:/var/run/docker.sock
      - \${CLAW_DATA_DIR}:/home/claw/.claw
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
      CLAW_EXECUTION_BACKEND: "\${CLAW_EXECUTION_BACKEND:-docker}"
      RUST_LOG: "\${RUST_LOG:-info}"
      HOME: /home/claw
      CLAW_HOST_DATA_DIR: "\${CLAW_DATA_DIR}"
      CLAW_HOST_CLAUDE_HOME: "\${CLAUDE_HOME}"
    depends_on:
      redis:
        condition: service_healthy
    volumes:
      - /var/run/docker.sock:/var/run/docker.sock
      - \${CLAUDE_HOME}:/home/claw/.claude
      - \${CLAUDE_JSON}:/home/claw/.claude.json:ro
      - \${CLAW_DATA_DIR}:/home/claw/.claw
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
echo "  On Linux, auth tokens are stored in ~/.claude/"
echo ""

if ! command -v claude &>/dev/null; then
  yellow "  Installing Claude Code CLI..."
  npm install -g @anthropic-ai/claude-code 2>&1 | tail -1
fi

if [ ! -d "$HOME/.claude" ] || [ ! -f "$HOME/.claude.json" ]; then
  echo "  Claude Code not authenticated. Launching login..."
  echo ""
  read -p "  Press Enter to start Claude login..."
  claude
  echo ""
else
  # Quick check if auth works
  if claude --version &>/dev/null; then
    green "  Claude Code authenticated ($(claude --version 2>/dev/null || echo 'installed'))"
  else
    yellow "  Claude Code installed but may need re-authentication."
    read -p "  Press Enter to start Claude login (or Ctrl+C to skip)..."
    claude
  fi
fi

if [ -d "$HOME/.claude" ]; then
  green "  ~/.claude/ exists — auth will be mounted into containers"
else
  red "  WARNING: ~/.claude/ not found. Worker will fail."
  red "  Run 'claude' manually to log in, then restart the worker."
fi

# --- Sandbox image ---
echo ""
echo "==========================================="
echo "  Setting up Docker sandbox image..."
echo "==========================================="
echo ""
# Pull the pre-built sandbox image from the registry
if $DOCKER pull "$REPO/sandbox:latest" 2>/dev/null; then
  $DOCKER tag "$REPO/sandbox:latest" claw-sandbox:latest
  green "  Sandbox image pulled and tagged as claw-sandbox:latest"
else
  yellow "  Could not pull sandbox image from registry."
  yellow "  This is expected on first install if the image is private."
  echo ""
  echo "  The sandbox image will need to be loaded manually:"
  echo "    Option 1: Build locally (requires the Dockerfile):"
  echo "      docker build -t claw-sandbox:latest -f docker/Dockerfile.sandbox ."
  echo "    Option 2: Load from a tar file:"
  echo "      docker load < sandbox.tar.gz"
  echo "    Option 3: Use local execution mode (no sandbox):"
  echo "      Edit .env and set CLAW_EXECUTION_BACKEND=local"
  echo ""
  read -p "  Continue without sandbox image? (worker will use local mode) [Y/n]: " SKIP_SANDBOX
  if [ "${SKIP_SANDBOX:-Y}" != "n" ] && [ "${SKIP_SANDBOX:-Y}" != "N" ]; then
    sed -i 's/CLAW_EXECUTION_BACKEND=docker/CLAW_EXECUTION_BACKEND=local/' .env
    yellow "  Set to local execution mode. Change to docker mode after loading sandbox image."
  fi
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

# Set execution_backend in Redis config (so Settings screen reflects it)
BACKEND=$(grep CLAW_EXECUTION_BACKEND .env | cut -d= -f2)
$DC --env-file "$ENV_FILE" exec -T api curl -sf \
  -X PUT http://localhost:8080/api/v1/config/execution_backend \
  -H "Content-Type: application/json" \
  -d "{\"value\":\"$BACKEND\"}" >/dev/null 2>&1 || true

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
echo "  Data dir:    $CLAW_DATA_DIR"
echo ""
echo "  Execution:  $BACKEND mode"
if [ "$BACKEND" = "docker" ]; then
  echo "  Jobs run in isolated claw-sandbox containers"
else
  echo "  Jobs run as direct subprocesses (set CLAW_EXECUTION_BACKEND=docker for isolation)"
fi
echo ""
echo "  TLS CA cert: $INSTALL_DIR/claw-ca.crt"
echo "  Install this cert on team machines to avoid browser warnings."
echo ""
echo "  Commands (run from $INSTALL_DIR):"
echo "    $DC --env-file $ENV_FILE logs -f worker"
echo "    $DC --env-file $ENV_FILE restart worker"
echo "    $DC --env-file $ENV_FILE down"
echo "==========================================="
