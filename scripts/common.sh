#!/bin/bash
# Shared config for claw scripts — sources .env and configures redis-cli

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

# Load .env
if [ -f "$PROJECT_DIR/.env" ]; then
  set -a
  source "$PROJECT_DIR/.env"
  set +a
fi

# Parse CLAW_REDIS_URL into redis-cli args
# Format: redis://[host]:[port]/[db]
CLAW_REDIS_URL="${CLAW_REDIS_URL:-redis://127.0.0.1:6379}"
REDIS_DB=$(echo "$CLAW_REDIS_URL" | grep -oE '/[0-9]+$' | tr -d '/')
REDIS_DB="${REDIS_DB:-0}"

rcli() {
  redis-cli -n "$REDIS_DB" --no-auth-warning "$@"
}
