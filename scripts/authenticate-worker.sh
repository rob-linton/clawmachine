#!/bin/bash
set -euo pipefail

echo ""
echo "=== ClaudeCodeClaw: Authenticate Worker ==="
echo ""
echo "This runs Claude Code interactively so you can complete OAuth login."
echo "The token will be saved to ~/.claude.json on this machine."
echo "The worker container mounts this file at runtime."
echo ""

# Check if claude is installed
if ! command -v claude &> /dev/null; then
    echo "Claude Code CLI not found. Installing..."
    npm install -g @anthropic-ai/claude-code
fi

# Check if already authenticated
if claude -p "say ok" --output-format stream-json 2>&1 | grep -q '"error":"authentication_failed"'; then
    echo "Not yet authenticated. Starting interactive login..."
    echo ""
    claude
    echo ""
    echo "Verifying authentication..."
    if claude -p "say ok" --output-format stream-json 2>&1 | grep -q '"error":"authentication_failed"'; then
        echo "ERROR: Authentication failed. Please try again."
        exit 1
    fi
fi

echo "Claude Code is authenticated."
echo ""
echo "Token location: ~/.claude.json"
echo ""
echo "You can now start the stack:"
echo "  docker compose -f docker/docker-compose.yml --env-file .env up -d"
echo ""
