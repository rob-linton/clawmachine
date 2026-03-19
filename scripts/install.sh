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

# Node.js check is only needed if Claude CLI needs to be installed on the host.
# The worker Docker image has its own Claude CLI, so host install is optional.

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
# Clean stale job dirs that may be owned by root (from Docker sandbox containers)
if [ -d "$CLAW_DATA_DIR/jobs" ]; then
  $DOCKER run --rm -v "$CLAW_DATA_DIR:/data" alpine rm -rf /data/jobs 2>/dev/null || true
fi
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

# --- Check Claude Code auth ---
echo ""
echo "==========================================="
echo "  Step: Claude Code Authentication"
echo "==========================================="
echo ""
echo "  The worker needs Claude Code authenticated on this machine."
echo "  Auth tokens are stored in ~/.claude/ (mounted into containers)."
echo ""

# Find claude binary (could be on PATH, in npm global, or in VSCode extension)
CLAUDE_BIN=""
if command -v claude &>/dev/null; then
  CLAUDE_BIN="claude"
else
  # Check common locations
  for loc in \
    "$HOME/.vscode/extensions/"anthropic.claude-code-*/resources/native-binary/claude \
    "$HOME/.vscode-server/extensions/"anthropic.claude-code-*/resources/native-binary/claude \
    /usr/local/bin/claude /usr/bin/claude; do
    if [ -x "$loc" ] 2>/dev/null; then
      CLAUDE_BIN="$loc"
      break
    fi
  done
fi

if [ -d "$HOME/.claude" ]; then
  green "  ~/.claude/ exists — auth tokens found"
  if [ -n "$CLAUDE_BIN" ]; then
    green "  Claude CLI: $CLAUDE_BIN"
  fi
else
  yellow "  ~/.claude/ not found — Claude Code needs to be authenticated."
  echo ""
  if [ -n "$CLAUDE_BIN" ]; then
    echo "  Found Claude at: $CLAUDE_BIN"
    read -p "  Press Enter to launch Claude for login..."
    "$CLAUDE_BIN"
  else
    echo "  Claude CLI not found on this machine."
    echo "  Options to authenticate:"
    echo "    1. Install Claude Code: npm install -g @anthropic-ai/claude-code && claude"
    echo "    2. Use Claude Code in VS Code/Cursor (it creates ~/.claude/ automatically)"
    echo "    3. Copy ~/.claude/ from another authenticated machine"
    echo ""
    read -p "  Press Enter once ~/.claude/ exists (or Ctrl+C to abort)..."
  fi
  echo ""
  if [ ! -d "$HOME/.claude" ]; then
    red "  WARNING: ~/.claude/ still not found. Worker auth will fail."
    red "  The worker will start but jobs will fail until authenticated."
  fi
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
$DC --env-file "$ENV_FILE" cp caddy:/data/caddy/pki/authorities/local/root.crt "/tmp/claw-ca.crt" 2>/dev/null && \
  cp /tmp/claw-ca.crt "$INSTALL_DIR/claw-ca.crt" 2>/dev/null && \
  rm /tmp/claw-ca.crt 2>/dev/null || true

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
