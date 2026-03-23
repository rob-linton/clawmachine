#!/bin/bash
set -euo pipefail
source "$(dirname "$0")/common.sh"

# Claw Machine Frontend — runs Flutter web in dev mode with hot reload
# All output goes to stdout AND log file via tee.
#
# Usage: ./scripts/frontend.sh           (start Flutter dev server on :3000)
#        ./scripts/frontend.sh build     (build release for production)
#
# Requires: backend.sh running in another terminal

FLUTTER_DIR="$PROJECT_DIR/flutter_ui"
LOG_DIR="$PROJECT_DIR/.logs"
mkdir -p "$LOG_DIR"

red()    { printf '\033[0;31m%s\033[0m\n' "$1"; }
green()  { printf '\033[0;32m%s\033[0m\n' "$1"; }
yellow() { printf '\033[0;33m%s\033[0m\n' "$1"; }

# Load frontend .env
ENV_FILE="$FLUTTER_DIR/.env.dev"
API_URL="http://localhost:8080"
if [ -f "$ENV_FILE" ]; then
    source "$ENV_FILE"
    echo "Loaded $ENV_FILE (API_URL=$API_URL)"
fi

# Kill any stale Flutter dev servers
pkill -f "flutter_tools.*run" 2>/dev/null || true
pkill -f "dart.*flutter" 2>/dev/null || true
pkill -f "Google Chrome.*--remote-debugging-port" 2>/dev/null || true
sleep 1

# Check backend is running
echo ""
echo "Checking backend at $API_URL ..."
if ! curl -s "$API_URL/api/v1/status" > /dev/null 2>&1; then
    red "  Backend not running at $API_URL"
    echo "  Start it first: ./scripts/backend.sh"
    exit 1
fi
green "  Backend: OK"

cd "$FLUTTER_DIR"

if [ "${1:-}" = "build" ]; then
    # Production build
    echo ""
    echo "Building Flutter web (release)..."
    flutter clean 2>&1 | tee "$LOG_DIR/build-flutter.log"
    flutter pub get 2>&1 | tee -a "$LOG_DIR/build-flutter.log"
    flutter build web --release --no-tree-shake-icons \
        --dart-define="API_URL=$API_URL" \
        2>&1 | tee -a "$LOG_DIR/build-flutter.log"
    green "  Build complete: flutter_ui/build/web/"
    green "  Build log: .logs/build-flutter.log"
else
    # Dev mode with hot reload
    echo ""
    echo "========================================="
    green "  Starting Flutter dev server"
    echo "  Frontend:  http://localhost:3000"
    echo "  Backend:   $API_URL"
    echo "  Log file:  .logs/flutter-dev.log"
    echo ""
    echo "  Press Ctrl+C to stop"
    echo "  Press 'r' in this terminal for hot reload"
    echo "  Press 'R' for hot restart"
    echo "========================================="
    echo ""

    flutter run -d chrome \
        --web-port=3000 \
        --dart-define="API_URL=$API_URL" \
        2>&1 | tee "$LOG_DIR/flutter-dev.log"
fi
