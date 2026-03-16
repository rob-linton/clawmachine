#!/bin/bash
set -euo pipefail
source "$(dirname "$0")/common.sh"

# ClaudeCodeClaw Flutter admin console
# Builds and serves the Flutter web UI via the API server.
#
# Usage: ./scripts/startup.sh           (build + open browser)
#        ./scripts/startup.sh --dev     (Flutter hot reload on :3000)
#        ./scripts/startup.sh --build   (build only, no browser)

red()    { printf '\033[0;31m%s\033[0m\n' "$1"; }
green()  { printf '\033[0;32m%s\033[0m\n' "$1"; }
yellow() { printf '\033[0;33m%s\033[0m\n' "$1"; }

MODE="${1:-}"
FLUTTER_DIR="$PROJECT_DIR/flutter_ui"

echo "========================================="
echo "  ClaudeCodeClaw Admin Console"
echo "========================================="
echo ""

# Check backend is running
if ! curl -s http://localhost:8080/api/v1/status > /dev/null 2>&1; then
    red "  Backend not running. Start it first:"
    echo "    ./scripts/dev.sh"
    exit 1
fi
green "  Backend: OK"

if [ "$MODE" = "--dev" ]; then
    # Hot reload mode — Flutter dev server on port 3000
    echo ""
    echo "Starting Flutter dev server (hot reload)..."
    echo "  Backend API:  http://localhost:8080"
    echo "  Flutter dev:  http://localhost:3000"
    echo ""
    yellow "  Note: Flutter dev server connects to the API at localhost:8080."
    yellow "  Make sure CORS is enabled (it is by default)."
    echo ""
    cd "$FLUTTER_DIR"
    flutter run -d chrome --web-port=3000

elif [ "$MODE" = "--build" ]; then
    # Build only
    echo ""
    echo "Building Flutter web..."
    cd "$FLUTTER_DIR"
    flutter build web --release 2>&1 | tail -3
    green "  Build complete: flutter_ui/build/web/"

else
    # Default: build + open in browser
    echo ""
    echo "Building Flutter web..."
    cd "$FLUTTER_DIR"
    flutter build web --release --no-tree-shake-icons 2>&1 | tail -1
    green "  Build: OK"

    # Restart API to serve the new build
    if [ -f "$PROJECT_DIR/.pids/api.pid" ]; then
        echo ""
        yellow "  Restarting API to serve new build..."
        API_PID=$(cat "$PROJECT_DIR/.pids/api.pid")
        kill "$API_PID" 2>/dev/null || true
        sleep 1
        cd "$PROJECT_DIR"
        cargo run -p claw-api > "$PROJECT_DIR/.logs/api.log" 2>&1 &
        echo $! > "$PROJECT_DIR/.pids/api.pid"
        # Wait for it
        for i in $(seq 1 10); do
            curl -s http://localhost:8080/api/v1/status > /dev/null 2>&1 && break
            sleep 1
        done
        green "  API restarted"
    fi

    # Verify it's being served
    echo ""
    if curl -s -o /dev/null -w "%{http_code}" http://localhost:8080/ | grep -q "200"; then
        green "  Admin console: http://localhost:8080"
    else
        red "  UI not being served. Check CLAW_STATIC_DIR in .env"
        exit 1
    fi

    # Open browser
    echo ""
    echo "Opening browser..."
    if command -v open > /dev/null 2>&1; then
        open http://localhost:8080
    elif command -v xdg-open > /dev/null 2>&1; then
        xdg-open http://localhost:8080
    else
        echo "  Open http://localhost:8080 in your browser"
    fi

    echo ""
    echo "========================================="
    green "  Admin console ready!"
    echo ""
    echo "  URL:        http://localhost:8080"
    echo "  Hot reload: ./scripts/startup.sh --dev"
    echo "  Rebuild:    ./scripts/startup.sh --build"
    echo "========================================="
fi
