#!/bin/bash
set -e
cd "$(dirname "$0")/.."

echo "=== ClaudeCodeClaw Deploy ==="

# Load .env if present
if [ -f .env ]; then
    set -a; source .env; set +a
fi

# Pull latest code
if git rev-parse --is-inside-work-tree > /dev/null 2>&1; then
    echo "Pulling latest code..."
    git pull
fi

# Build and restart containers (Flutter is built inside Docker)
echo "Building containers..."
docker compose -f docker/docker-compose.yml --env-file .env build

echo "Starting services..."
docker compose -f docker/docker-compose.yml --env-file .env up -d

# Wait for API to be healthy
echo "Waiting for API to be ready..."
for i in $(seq 1 30); do
    if curl -sf http://localhost:8080/api/v1/status > /dev/null 2>&1; then
        echo ""
        echo "=== Deploy complete ==="
        echo "  Access: https://${CLAW_HOST_IP:-localhost}"
        echo ""
        echo "  To extract the CA cert for team distribution:"
        echo "    docker compose -f docker/docker-compose.yml --env-file .env cp caddy:/data/caddy/pki/authorities/local/root.crt ./claw-ca.crt"
        echo ""
        exit 0
    fi
    printf "."
    sleep 2
done

echo ""
echo "WARNING: API did not become healthy within 60 seconds"
echo "Check logs: docker compose -f docker/docker-compose.yml logs"
exit 1
