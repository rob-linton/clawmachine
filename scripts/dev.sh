#!/bin/bash
set -euo pipefail
source "$(dirname "$0")/common.sh"

# ClaudeCodeClaw dev startup — starts Redis, API, Worker, Scheduler
# Usage: ./scripts/dev.sh        (start all)
#        ./scripts/dev.sh stop   (stop all)

PIDS_DIR="$PROJECT_DIR/.pids"
LOG_DIR="$PROJECT_DIR/.logs"
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
    green "All services stopped."
}

if [ "${1:-}" = "stop" ]; then
    stop_all
    exit 0
fi

# Stop any existing instances first
stop_all 2>/dev/null || true

echo "========================================="
echo "  ClaudeCodeClaw Dev Environment"
echo "========================================="
echo ""

# 1. Check Redis
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

# 2. Build
echo ""
echo "Building workspace..."
cargo build --workspace 2>&1 | tail -1
green "  Build: OK"

# 3. Build Flutter web (if source is newer than build)
FLUTTER_BUILD="$PROJECT_DIR/flutter_ui/build/web/index.html"
FLUTTER_SRC="$PROJECT_DIR/flutter_ui/lib/main.dart"
if [ ! -f "$FLUTTER_BUILD" ] || [ "$FLUTTER_SRC" -nt "$FLUTTER_BUILD" ]; then
    echo ""
    echo "Building Flutter web..."
    (cd "$PROJECT_DIR/flutter_ui" && flutter build web --release 2>&1 | tail -1)
    green "  Flutter: OK"
else
    green "  Flutter: Up to date"
fi

# 4. Start API
echo ""
echo "Starting services..."
cargo run -p claw-api > "$LOG_DIR/api.log" 2>&1 &
echo $! > "$PIDS_DIR/api.pid"
echo "  API server:  pid $! (log: .logs/api.log)"

# 5. Start Worker
cargo run -p claw-worker > "$LOG_DIR/worker.log" 2>&1 &
echo $! > "$PIDS_DIR/worker.pid"
echo "  Worker:      pid $! (log: .logs/worker.log)"

# 6. Start Scheduler
cargo run -p claw-scheduler > "$LOG_DIR/scheduler.log" 2>&1 &
echo $! > "$PIDS_DIR/scheduler.pid"
echo "  Scheduler:   pid $! (log: .logs/scheduler.log)"

# Wait for API to be ready
echo ""
printf "Waiting for API..."
for i in $(seq 1 20); do
    if curl -s http://localhost:8080/api/v1/status > /dev/null 2>&1; then
        break
    fi
    printf "."
    sleep 1
done
echo ""

if curl -s http://localhost:8080/api/v1/status > /dev/null 2>&1; then
    green "  API ready at http://localhost:8080"
else
    red "  API failed to start. Check .logs/api.log"
    exit 1
fi

echo ""
echo "========================================="
green "  All services running!"
echo ""
echo "  UI:        http://localhost:8080"
echo "  API:       http://localhost:8080/api/v1/status"
echo "  Logs:      .logs/{api,worker,scheduler}.log"
echo ""
echo "  Submit:    claw submit \"your prompt\""
echo "  Stop:      ./scripts/dev.sh stop"
echo "========================================="
