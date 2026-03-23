#!/bin/bash
set -euo pipefail
source "$(dirname "$0")/common.sh"

# Claw Machine — start everything (backend + Flutter admin console)
#
# Usage: ./scripts/startup.sh           (start all, open browser)
#        ./scripts/startup.sh stop      (stop all)
#        ./scripts/startup.sh --dev     (backend + Flutter hot reload on :3000)
#        ./scripts/startup.sh logs      (tail all log files)

PIDS_DIR="$PROJECT_DIR/.pids"
LOG_DIR="$PROJECT_DIR/.logs"
FLUTTER_DIR="$PROJECT_DIR/flutter_ui"
mkdir -p "$PIDS_DIR" "$LOG_DIR"

red()    { printf '\033[0;31m%s\033[0m\n' "$1"; }
green()  { printf '\033[0;32m%s\033[0m\n' "$1"; }
yellow() { printf '\033[0;33m%s\033[0m\n' "$1"; }

stop_all() {
    echo "Stopping services..."
    # Kill by PID file
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
    # Also kill any stale processes by name (catches orphans)
    pkill -f "target/debug/claw-api" 2>/dev/null && echo "  Killed stale claw-api" || true
    pkill -f "target/debug/claw-worker" 2>/dev/null && echo "  Killed stale claw-worker" || true
    pkill -f "target/debug/claw-scheduler" 2>/dev/null && echo "  Killed stale claw-scheduler" || true
    sleep 1
    green "Stopped."
}

if [ "${1:-}" = "stop" ]; then
    stop_all
    exit 0
fi

if [ "${1:-}" = "logs" ]; then
    echo "Tailing all logs (Ctrl+C to stop)..."
    tail -f "$LOG_DIR"/*.log
    exit 0
fi

# Stop any existing instances first
stop_all 2>/dev/null || true

echo "========================================="
echo "  Claw Machine"
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
cargo build --workspace > "$LOG_DIR/build-rust.log" 2>&1
if [ $? -ne 0 ]; then
    red "  Rust build failed! See .logs/build-rust.log"
    tail -20 "$LOG_DIR/build-rust.log"
    exit 1
fi
green "  Rust: OK (log: .logs/build-rust.log)"

# --- 3. Build Flutter ---
echo ""
echo "Building Flutter web..."
cd "$FLUTTER_DIR"
flutter clean > "$LOG_DIR/build-flutter.log" 2>&1
flutter pub get >> "$LOG_DIR/build-flutter.log" 2>&1
flutter build web --release --no-tree-shake-icons >> "$LOG_DIR/build-flutter.log" 2>&1
if [ $? -ne 0 ]; then
    red "  Flutter build failed! See .logs/build-flutter.log"
    tail -20 "$LOG_DIR/build-flutter.log"
    exit 1
fi
green "  Flutter: OK (log: .logs/build-flutter.log)"

# --- 4. Start services with debug logging ---
echo ""
echo "Starting services..."
cd "$PROJECT_DIR"

# Set verbose logging for development
export RUST_LOG="${RUST_LOG:-info,claw_api=debug,claw_worker=debug,claw_scheduler=debug,claw_redis=debug}"

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
    tail -20 "$LOG_DIR/api.log"
    exit 1
fi

# --- 6. Verify UI ---
if curl -s -o /dev/null -w "%{http_code}" http://localhost:8080/ | grep -q "200"; then
    green "  Admin console: http://localhost:8080"
else
    red "  UI not being served. Check .logs/api.log"
    tail -10 "$LOG_DIR/api.log"
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
    flutter run -d chrome --web-port=3000 2>&1 | tee "$LOG_DIR/flutter-dev.log"
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
    green "  Claw Machine running!"
    echo ""
    echo "  URL:        http://localhost:8080"
    echo "  Logs:       .logs/{api,worker,scheduler}.log"
    echo "  Builds:     .logs/{build-rust,build-flutter}.log"
    echo ""
    echo "  Tail logs:  ./scripts/startup.sh logs"
    echo "  Stop:       ./scripts/startup.sh stop"
    echo "  Hot reload: ./scripts/startup.sh --dev"
    echo "========================================="
fi
