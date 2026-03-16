#!/bin/bash
set -euo pipefail
source "$(dirname "$0")/common.sh"

# ClaudeCodeClaw — start everything (backend + Flutter admin console)
#
# Usage: ./scripts/startup.sh           (start all, open browser)
#        ./scripts/startup.sh stop      (stop all)
#        ./scripts/startup.sh --dev     (backend + Flutter hot reload on :3000)

PIDS_DIR="$PROJECT_DIR/.pids"
LOG_DIR="$PROJECT_DIR/.logs"
FLUTTER_DIR="$PROJECT_DIR/flutter_ui"
mkdir -p "$PIDS_DIR" "$LOG_DIR"

red()    { printf '\033[0;31m%s\033[0m\n' "$1"; }
green()  { printf '\033[0;32m%s\033[0m\n' "$1"; }
yellow() { printf '\033[0;33m%s\033[0m\n' "$1"; }

stop_all() {
    echo "Stopping services..."
    for pidfile in "$PIDS_DIR"/*.pid; do
        [ -f "$pidfile" ] || continue
        pid=$(cat "$pidfile")
        name=$(basename "$pidfile" .pid)
        if kill -0 "$pid" 2>/dev/null; then
            kill "$pid" 2>/dev/null
            echo "  Stopped $name (pid $pid)"
        fi
        rm -f "$pidfile"
    done
    green "Stopped."
}

if [ "${1:-}" = "stop" ]; then
    stop_all
    exit 0
fi

# Stop any existing instances first
stop_all 2>/dev/null || true

echo "========================================="
echo "  ClaudeCodeClaw"
echo "========================================="
echo ""

# --- 1. Redis ---
echo "Checking Redis..."
if ! rcli ping > /dev/null 2>&1; then
    yellow "  Redis not running. Starting via Docker..."
    docker run -d --name claw-redis -p 6379:6379 redis:7-alpine > /dev/null 2>&1 || true
    sleep 2
    if ! rcli ping > /dev/null 2>&1; then
        red "  Failed to start Redis. Please start it manually."
        exit 1
    fi
fi
green "  Redis: OK (DB $REDIS_DB)"

# --- 2. Build Rust ---
echo ""
echo "Building Rust workspace..."
cd "$PROJECT_DIR"
if ! cargo build --workspace 2>&1 | tail -3; then
    red "  Rust build failed!"
    exit 1
fi
green "  Rust: OK"

# --- 3. Build Flutter ---
echo ""
echo "Building Flutter web..."
cd "$FLUTTER_DIR"
flutter clean > /dev/null 2>&1
flutter pub get > /dev/null 2>&1
if ! flutter build web --release --no-tree-shake-icons 2>&1 | tail -3; then
    red "  Flutter build failed!"
    exit 1
fi
green "  Flutter: OK"

# --- 4. Start services ---
echo ""
echo "Starting services..."
cd "$PROJECT_DIR"

nohup cargo run -p claw-api > "$LOG_DIR/api.log" 2>&1 &
echo $! > "$PIDS_DIR/api.pid"
echo "  API server:  pid $! (log: .logs/api.log)"

nohup cargo run -p claw-worker > "$LOG_DIR/worker.log" 2>&1 &
echo $! > "$PIDS_DIR/worker.pid"
echo "  Worker:      pid $! (log: .logs/worker.log)"

nohup cargo run -p claw-scheduler > "$LOG_DIR/scheduler.log" 2>&1 &
echo $! > "$PIDS_DIR/scheduler.pid"
echo "  Scheduler:   pid $! (log: .logs/scheduler.log)"

# --- 5. Wait for API ---
echo ""
printf "Waiting for API..."
for i in $(seq 1 30); do
    if curl -s http://localhost:8080/api/v1/status > /dev/null 2>&1; then
        break
    fi
    printf "."
    sleep 1
done
echo ""

if ! curl -s http://localhost:8080/api/v1/status > /dev/null 2>&1; then
    red "  API failed to start. Check .logs/api.log"
    exit 1
fi

# --- 6. Verify UI ---
if curl -s -o /dev/null -w "%{http_code}" http://localhost:8080/ | grep -q "200"; then
    green "  Admin console: http://localhost:8080"
else
    red "  UI not being served. Check CLAW_STATIC_DIR in .env"
    exit 1
fi

# --- 7. Open browser (unless --dev mode) ---
if [ "${1:-}" = "--dev" ]; then
    echo ""
    echo "Starting Flutter dev server (hot reload)..."
    echo "  Backend API:  http://localhost:8080"
    echo "  Flutter dev:  http://localhost:3000"
    echo ""
    cd "$FLUTTER_DIR"
    flutter run -d chrome --web-port=3000
else
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
    green "  ClaudeCodeClaw running!"
    echo ""
    echo "  URL:        http://localhost:8080"
    echo "  Logs:       .logs/{api,worker,scheduler}.log"
    echo ""
    echo "  Stop:       ./scripts/startup.sh stop"
    echo "  Hot reload: ./scripts/startup.sh --dev"
    echo "========================================="
fi
