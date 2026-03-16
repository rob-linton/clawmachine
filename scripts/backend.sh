#!/bin/bash
set -euo pipefail
source "$(dirname "$0")/common.sh"

# ClaudeCodeClaw Backend — runs API, Worker, Scheduler in foreground
# All output goes to stdout AND log files via tee.
#
# Usage: ./scripts/backend.sh        (start all backend services)
#        ./scripts/backend.sh stop   (stop all)

LOG_DIR="$PROJECT_DIR/.logs"
PIDS_DIR="$PROJECT_DIR/.pids"
mkdir -p "$LOG_DIR" "$PIDS_DIR"

red()    { printf '\033[0;31m%s\033[0m\n' "$1"; }
green()  { printf '\033[0;32m%s\033[0m\n' "$1"; }
yellow() { printf '\033[0;33m%s\033[0m\n' "$1"; }

stop_all() {
    echo "Stopping backend..."
    pkill -f "target/debug/claw-api" 2>/dev/null && echo "  Stopped API" || true
    pkill -f "target/debug/claw-worker" 2>/dev/null && echo "  Stopped Worker" || true
    pkill -f "target/debug/claw-scheduler" 2>/dev/null && echo "  Stopped Scheduler" || true
    rm -f "$PIDS_DIR"/*.pid
    sleep 1
    green "Backend stopped."
}

if [ "${1:-}" = "stop" ]; then
    stop_all
    exit 0
fi

# Stop stale processes
stop_all 2>/dev/null || true

echo "========================================="
echo "  ClaudeCodeClaw Backend"
echo "========================================="
echo ""

# --- 1. Redis ---
echo "Checking Redis..."
if ! rcli ping > /dev/null 2>&1; then
    yellow "  Redis not running. Starting via Docker..."
    docker run -d --name claw-redis -p 6379:6379 redis:7-alpine > /dev/null 2>&1 || true
    sleep 2
    if ! rcli ping > /dev/null 2>&1; then
        red "  Failed to start Redis."
        exit 1
    fi
fi
green "  Redis: OK (DB $REDIS_DB)"

# --- 2. Build ---
echo ""
echo "Building Rust workspace..."
cd "$PROJECT_DIR"
if ! cargo build --workspace 2>&1 | tee "$LOG_DIR/build-rust.log" | tail -5; then
    red "  Build failed! See .logs/build-rust.log"
    exit 1
fi
green "  Build: OK"

# --- 3. Set dev log level ---
export RUST_LOG="${RUST_LOG:-info,claw_api=debug,claw_worker=debug,claw_scheduler=debug,claw_redis=debug}"

# --- 4. Start services with tee (stdout + log file) ---
echo ""
echo "========================================="
green "  Starting backend services..."
echo "  API log:       .logs/api.log"
echo "  Worker log:    .logs/worker.log"
echo "  Scheduler log: .logs/scheduler.log"
echo "  API:           http://localhost:8080"
echo ""
echo "  Press Ctrl+C to stop all services"
echo "========================================="
echo ""

# Start each service in background, tee output to both terminal and log file
cargo run -p claw-api 2>&1 | tee "$LOG_DIR/api.log" &
API_PID=$!
echo $API_PID > "$PIDS_DIR/api.pid"

cargo run -p claw-worker 2>&1 | tee "$LOG_DIR/worker.log" &
WORKER_PID=$!
echo $WORKER_PID > "$PIDS_DIR/worker.pid"

cargo run -p claw-scheduler 2>&1 | tee "$LOG_DIR/scheduler.log" &
SCHEDULER_PID=$!
echo $SCHEDULER_PID > "$PIDS_DIR/scheduler.pid"

# Wait for API to be ready
printf "Waiting for API..."
for i in $(seq 1 30); do
    if curl -s http://localhost:8080/api/v1/status > /dev/null 2>&1; then
        break
    fi
    printf "."
    sleep 1
done
echo ""

if curl -s http://localhost:8080/api/v1/status > /dev/null 2>&1; then
    green "API ready at http://localhost:8080"
else
    red "API failed to start!"
fi

echo ""
echo "--- All output below is live from API + Worker + Scheduler ---"
echo ""

# Wait for Ctrl+C, then clean up
cleanup() {
    echo ""
    yellow "Shutting down..."
    kill $API_PID $WORKER_PID $SCHEDULER_PID 2>/dev/null
    rm -f "$PIDS_DIR"/*.pid
    wait 2>/dev/null
    green "Backend stopped."
    exit 0
}
trap cleanup INT TERM

# Wait for all background processes
wait
