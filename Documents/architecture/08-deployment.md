# Deployment — Docker Compose, Configuration, and Operations

## 1. Deployment Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                    Docker Compose Stack                          │
│                                                                  │
│  ┌─────────────────────────────────────────────────────────┐   │
│  │  redis (redis:7-alpine)                                  │   │
│  │  Port: 6379 (internal), optionally exposed               │   │
│  │  Volume: redis_data (AOF persistence)                    │   │
│  └─────────────────────────────────────────────────────────┘   │
│                              │                                   │
│                              │ redis:6379                         │
│           ┌──────────────────┼──────────────────┐                │
│           │                  │                  │                │
│  ┌────────┴──────┐  ┌───────┴───────┐  ┌──────┴───────┐       │
│  │  claw-api     │  │  claw-worker  │  │claw-scheduler│       │
│  │               │  │               │  │              │       │
│  │  Port: 8080   │  │  ANTHROPIC_   │  │  Watches     │       │
│  │  (exposed)    │  │  API_KEY      │  │  ./jobs/     │       │
│  │               │  │               │  │              │       │
│  │  Serves:      │  │  Runs:        │  │  Cron +      │       │
│  │  - REST API   │  │  - 2 workers  │  │  Filewatcher │       │
│  │  - WebSocket  │  │  - claude -p  │  │              │       │
│  │  - Flutter UI │  │               │  │              │       │
│  └───────────────┘  └───────────────┘  └──────────────┘       │
│                                                                  │
│  Volumes:                                                        │
│  - redis_data    → /data (AOF persistence)                       │
│  - ./output      → /app/output (job results)                     │
│  - ./skills      → /app/skills (skill files)                     │
│  - ./jobs        → /app/jobs (file watcher input)                │
│  - ./workspaces  → /app/workspaces (claude working dirs)         │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

## 2. Docker Compose Configuration

```yaml
# docker/docker-compose.yml

services:
  redis:
    image: redis:7-alpine
    restart: unless-stopped
    ports:
      - "${CLAW_REDIS_PORT:-6379}:6379"
    volumes:
      - redis_data:/data
    command: >
      redis-server
      --appendonly yes
      --appendfsync everysec
      --maxmemory 512mb
      --maxmemory-policy noeviction
    healthcheck:
      test: ["CMD", "redis-cli", "ping"]
      interval: 5s
      timeout: 3s
      retries: 5
      start_period: 5s

  api:
    build:
      context: ..
      dockerfile: docker/Dockerfile.backend
      target: api
    restart: unless-stopped
    ports:
      - "${CLAW_API_PORT:-8080}:8080"
    environment:
      CLAW_REDIS_URL: redis://redis:6379
      CLAW_API_PORT: "8080"
      CLAW_STATIC_DIR: /app/static
      CLAW_CORS_ORIGINS: "${CLAW_CORS_ORIGINS:-*}"
      RUST_LOG: "${RUST_LOG:-info,claw_api=debug}"
    depends_on:
      redis:
        condition: service_healthy
    volumes:
      - ../output:/app/output
      - ../skills:/app/skills
    healthcheck:
      test: ["CMD", "curl", "-f", "http://localhost:8080/api/v1/status"]
      interval: 10s
      timeout: 5s
      retries: 3
      start_period: 10s

  worker:
    build:
      context: ..
      dockerfile: docker/Dockerfile.backend
      target: worker
    restart: unless-stopped
    environment:
      CLAW_REDIS_URL: redis://redis:6379
      CLAW_WORKER_CONCURRENCY: "${CLAW_WORKER_CONCURRENCY:-2}"
      CLAW_WORKER_TIMEOUT_SECS: "${CLAW_WORKER_TIMEOUT:-1800}"
      CLAW_SKILLS_DIR: /app/skills
      CLAW_OUTPUT_DIR: /app/output
      # Claude Code uses OAuth — mount the host user's ~/.claude/ for tokens
      # No ANTHROPIC_API_KEY needed when using OAuth auth
      RUST_LOG: "${RUST_LOG:-info,claw_worker=debug}"
      HOME: /home/claw
    depends_on:
      redis:
        condition: service_healthy
    volumes:
      - ../output:/app/output
      - ../skills:/app/skills
      - ../workspaces:/app/workspaces
      - ${HOME}/.claude:/home/claw/.claude:ro  # Mount OAuth tokens from host
    deploy:
      replicas: ${CLAW_WORKER_REPLICAS:-1}

  scheduler:
    build:
      context: ..
      dockerfile: docker/Dockerfile.backend
      target: scheduler
    restart: unless-stopped
    environment:
      CLAW_REDIS_URL: redis://redis:6379
      CLAW_JOBS_WATCH_DIR: /app/jobs
      CLAW_CRON_SYNC_INTERVAL_SECS: "${CLAW_CRON_SYNC:-60}"
      RUST_LOG: "${RUST_LOG:-info,claw_scheduler=debug}"
    depends_on:
      redis:
        condition: service_healthy
    volumes:
      - ../jobs:/app/jobs

volumes:
  redis_data:
    driver: local
```

## 3. Multi-Stage Dockerfile

```dockerfile
# docker/Dockerfile.backend

# ============================================
# Stage 1: Build all Rust binaries
# ============================================
FROM rust:1.83-bookworm AS builder

WORKDIR /build

# Cache dependencies by copying manifests first
COPY Cargo.toml Cargo.lock ./
COPY crates/claw-models/Cargo.toml crates/claw-models/
COPY crates/claw-redis/Cargo.toml crates/claw-redis/
COPY crates/claw-api/Cargo.toml crates/claw-api/
COPY crates/claw-worker/Cargo.toml crates/claw-worker/
COPY crates/claw-scheduler/Cargo.toml crates/claw-scheduler/
COPY crates/claw-cli/Cargo.toml crates/claw-cli/

# Create dummy source files for dependency caching
RUN mkdir -p crates/claw-models/src && echo "pub fn dummy() {}" > crates/claw-models/src/lib.rs && \
    mkdir -p crates/claw-redis/src && echo "pub fn dummy() {}" > crates/claw-redis/src/lib.rs && \
    mkdir -p crates/claw-api/src && echo "fn main() {}" > crates/claw-api/src/main.rs && \
    mkdir -p crates/claw-worker/src && echo "fn main() {}" > crates/claw-worker/src/main.rs && \
    mkdir -p crates/claw-scheduler/src && echo "fn main() {}" > crates/claw-scheduler/src/main.rs && \
    mkdir -p crates/claw-cli/src && echo "fn main() {}" > crates/claw-cli/src/main.rs

# Build dependencies only (cached layer)
RUN cargo build --release 2>/dev/null || true

# Now copy actual source code
COPY crates/ crates/

# Touch source files to invalidate cache for actual compilation
RUN find crates -name "*.rs" -exec touch {} +

# Build all binaries
RUN cargo build --release --workspace

# ============================================
# Stage 2: Flutter web build
# ============================================
FROM ghcr.io/cirruslabs/flutter:stable AS flutter-builder

WORKDIR /build/flutter_ui
COPY flutter_ui/ .
RUN flutter pub get && flutter build web --release --no-tree-shake-icons

# ============================================
# Stage 3: API server image
# ============================================
FROM debian:bookworm-slim AS api

RUN apt-get update && \
    apt-get install -y --no-install-recommends ca-certificates curl && \
    rm -rf /var/lib/apt/lists/*

# Create non-root user
RUN useradd -m -s /bin/bash claw
USER claw

COPY --from=builder /build/target/release/claw-api /usr/local/bin/
COPY --from=flutter-builder /build/flutter_ui/build/web /app/static

EXPOSE 8080
CMD ["claw-api"]

# ============================================
# Stage 4: Worker image
# ============================================
FROM debian:bookworm-slim AS worker

RUN apt-get update && \
    apt-get install -y --no-install-recommends \
        ca-certificates \
        curl \
        git \
        nodejs \
        npm \
    && rm -rf /var/lib/apt/lists/*

# Install Claude Code CLI (pinned version for reproducibility)
# Update this version after testing compatibility with new releases
ARG CLAUDE_CODE_VERSION=1.0.0
RUN npm install -g @anthropic-ai/claude-code@${CLAUDE_CODE_VERSION}

# Create non-root user with home directory (claude needs it)
RUN useradd -m -s /bin/bash claw
USER claw

# Claude Code OAuth tokens are mounted at runtime from the host's ~/.claude/
# The host user must have completed OAuth login via `claude` before starting workers
# Alternatively, set ANTHROPIC_API_KEY env var for API key auth

COPY --from=builder /build/target/release/claw-worker /usr/local/bin/

CMD ["claw-worker"]

# ============================================
# Stage 5: Scheduler image
# ============================================
FROM debian:bookworm-slim AS scheduler

RUN apt-get update && \
    apt-get install -y --no-install-recommends ca-certificates && \
    rm -rf /var/lib/apt/lists/*

RUN useradd -m -s /bin/bash claw
USER claw

COPY --from=builder /build/target/release/claw-scheduler /usr/local/bin/

CMD ["claw-scheduler"]

# ============================================
# Stage 6: CLI image (for distribution)
# ============================================
FROM debian:bookworm-slim AS cli

RUN apt-get update && \
    apt-get install -y --no-install-recommends ca-certificates && \
    rm -rf /var/lib/apt/lists/*

COPY --from=builder /build/target/release/claw-cli /usr/local/bin/claw

ENTRYPOINT ["claw"]
```

## 4. Environment File

```bash
# .env (used by docker compose)

# Claude Code auth: OAuth-based (no API key needed)
# The worker container mounts ~/.claude/ from the host for OAuth tokens.
# Ensure you have run `claude` locally and completed OAuth login first.

# Optional overrides
CLAW_API_PORT=8080
CLAW_REDIS_PORT=6379
CLAW_WORKER_CONCURRENCY=2
CLAW_WORKER_REPLICAS=1
CLAW_WORKER_TIMEOUT=1800
CLAW_CRON_SYNC=60
CLAW_CORS_ORIGINS=*
RUST_LOG=info
```

## 5. Operations

### 5.1 Starting the Stack

```bash
# First time: build and start
docker compose -f docker/docker-compose.yml up --build -d

# Subsequent starts
docker compose -f docker/docker-compose.yml up -d

# View logs
docker compose -f docker/docker-compose.yml logs -f

# View specific service logs
docker compose -f docker/docker-compose.yml logs -f worker
```

### 5.2 Scaling Workers

```bash
# Scale to 3 worker containers (each running CLAW_WORKER_CONCURRENCY tasks)
docker compose -f docker/docker-compose.yml up -d --scale worker=3

# Or set in .env:
CLAW_WORKER_REPLICAS=3
```

With 3 worker containers at concurrency=2, you get 6 parallel worker tasks.

### 5.3 Stopping

```bash
# Graceful stop (SIGTERM → workers finish current jobs)
docker compose -f docker/docker-compose.yml stop

# Stop and remove containers
docker compose -f docker/docker-compose.yml down

# Stop and remove containers + volumes (deletes all data!)
docker compose -f docker/docker-compose.yml down -v
```

### 5.4 Updating

```bash
# Rebuild and restart (zero-downtime for API if using rolling update)
docker compose -f docker/docker-compose.yml up --build -d

# Rebuild only worker
docker compose -f docker/docker-compose.yml build worker
docker compose -f docker/docker-compose.yml up -d worker
```

### 5.5 Backup and Restore

**Redis data** is persisted via AOF to the `redis_data` volume.

```bash
# Backup Redis data
docker compose -f docker/docker-compose.yml exec redis redis-cli BGSAVE
docker cp $(docker compose -f docker/docker-compose.yml ps -q redis):/data/dump.rdb ./backup/

# Or backup the AOF file
docker cp $(docker compose -f docker/docker-compose.yml ps -q redis):/data/appendonly.aof ./backup/

# Restore: copy file back to volume before starting
```

**Job outputs** are on the host filesystem (`./output/`), so standard file backup applies.

### 5.6 Monitoring Health

```bash
# Check all services
docker compose -f docker/docker-compose.yml ps

# API health check
curl http://localhost:8080/api/v1/status

# Redis health
docker compose -f docker/docker-compose.yml exec redis redis-cli info stats

# Worker status
curl http://localhost:8080/api/v1/workers
# or
claw workers
```

## 6. Local Development Setup

For development, you don't need Docker for the Rust services — run them directly:

### 6.1 Prerequisites

- Rust 1.83+ (`rustup update`)
- Flutter 3.24+ (`flutter upgrade`)
- Redis 7+ (local install or Docker)
- Claude Code CLI (`npm install -g @anthropic-ai/claude-code`)
- Claude Code CLI authenticated via OAuth (`claude` has been run and login completed)

### 6.2 Start Redis

```bash
# Option A: Docker (recommended)
docker run -d --name claw-redis -p 6379:6379 redis:7-alpine

# Option B: Local install (macOS)
brew install redis && redis-server
```

### 6.3 Start Backend Services

Each in a separate terminal:

```bash
# Terminal 1: API server
cargo run -p claw-api

# Terminal 2: Worker
cargo run -p claw-worker  # Uses host user's OAuth session automatically

# Terminal 3: Scheduler
cargo run -p claw-scheduler

# Terminal 4: Flutter dev server (with hot reload)
cd flutter_ui && flutter run -d chrome
```

### 6.4 Use the CLI

```bash
# From the workspace root
cargo run -p claw-cli -- submit "hello world"
cargo run -p claw-cli -- status
cargo run -p claw-cli -- list

# Or install it
cargo install --path crates/claw-cli
claw submit "hello world"
```

## 7. Configuration Reference

### 7.1 All Environment Variables

| Variable | Service | Default | Description |
|----------|---------|---------|-------------|
| `CLAW_REDIS_URL` | all | `redis://127.0.0.1:6379` | Redis connection URL |
| `RUST_LOG` | all | `info` | Log level filter |
| `CLAW_API_PORT` | api | `8080` | HTTP listen port |
| `CLAW_STATIC_DIR` | api | `./static` | Flutter web build path |
| `CLAW_CORS_ORIGINS` | api | `*` | Allowed CORS origins |
| `CLAW_WORKER_CONCURRENCY` | worker | `2` | Async tasks per worker process |
| `CLAW_WORKER_TIMEOUT_SECS` | worker | `1800` | Default job timeout |
| `CLAW_SKILLS_DIR` | worker | `./skills` | Skills filesystem directory |
| `CLAW_OUTPUT_DIR` | worker | `./output` | Default file output directory |
| (OAuth session) | worker | (required) | Claude Code OAuth tokens mounted from host `~/.claude/` |
| `CLAW_JOBS_WATCH_DIR` | scheduler | `./jobs` | Directory for .job files |
| `CLAW_CRON_SYNC_INTERVAL_SECS` | scheduler | `60` | Cron definition re-sync interval |
| `CLAW_FAILURE_WEBHOOK_URL` | worker | (none) | Webhook URL to POST on job failure (Slack, PagerDuty, etc.) |
| `CLAW_CLAUDE_CODE_VERSION` | worker | (build arg) | Expected Claude Code CLI version (logs warning on mismatch) |

### 7.2 Configuration File (`config.toml`)

```toml
# config.toml — loaded by all services, overridden by env vars

[redis]
url = "redis://127.0.0.1:6379"
pool_size = 10                      # Connection pool size per service

[api]
port = 8080
static_dir = "./flutter_ui/build/web"
cors_origins = ["http://localhost:3000"]

[worker]
concurrency = 2
timeout_secs = 1800
skills_dir = "./skills"
output_dir = "./output"
workspaces_dir = "./workspaces"
reaper_interval_secs = 15
heartbeat_interval_secs = 10
heartbeat_ttl_secs = 30
max_retries = 3

[scheduler]
watch_dir = "./jobs"
cron_sync_interval_secs = 60

[defaults]
model = "sonnet"
max_budget_usd = 1.00
priority = 5
output = "redis"
```

### 7.3 Precedence

1. Environment variables (highest priority)
2. Config file specified by `--config` flag
3. `./claw.toml` (project-local)
4. `~/.claw/config.toml` (user-global)
5. Hardcoded defaults (lowest priority)

## 8. Graceful Shutdown

All services handle SIGTERM gracefully:

### 8.1 API Server

1. Stop accepting new connections
2. Wait for in-flight HTTP requests to complete (30s timeout)
3. Close all WebSocket connections
4. Disconnect from Redis
5. Exit 0

### 8.2 Worker

1. Set shutdown flag (AtomicBool)
2. Worker tasks stop claiming new jobs
3. Wait for in-flight jobs to complete (configurable timeout, default 5 minutes)
4. If timeout exceeded, send SIGTERM to claude child processes
5. Wait 10s for claude to exit
6. If still running, SIGKILL
7. Delete heartbeat keys from Redis
8. Exit 0

### 8.3 Scheduler

1. Stop cron engine (no new triggers)
2. Stop file watcher
3. Wait for any in-flight job submissions to Redis
4. Exit 0

## 9. Logging

All services use `tracing` with `tracing-subscriber`:

### 9.1 Development

Human-readable colored output:

```
2026-03-15T22:30:02.123Z  INFO claw_worker::executor: Job claimed job_id=f47ac10b worker_id=worker-1-task-0
2026-03-15T22:30:02.456Z DEBUG claw_worker::prompt_builder: Building prompt skills=["code-review"] tags=["rust"]
2026-03-15T22:30:03.789Z  INFO claw_worker::executor: Spawning claude -p model=sonnet working_dir=/repos/project
2026-03-15T22:32:15.012Z  INFO claw_worker::executor: Job completed job_id=f47ac10b cost_usd=0.42 duration_ms=193000
```

### 9.2 Production (Docker)

JSON lines format for log aggregation:

```json
{"timestamp":"2026-03-15T22:30:02.123Z","level":"INFO","target":"claw_worker::executor","message":"Job claimed","job_id":"f47ac10b","worker_id":"worker-1-task-0"}
```

Configured via `RUST_LOG`:

```bash
# All info, worker debug
RUST_LOG=info,claw_worker=debug

# Everything debug
RUST_LOG=debug

# Just errors
RUST_LOG=error
```

## 10. Security Considerations

### 10.1 Authentication Protection

- Claude Code uses OAuth by default — tokens live in `~/.claude/` on the host
- The worker container mounts `~/.claude/` as read-only
- OAuth tokens are never logged, never stored in Redis
- Alternative: if using API key auth (`ANTHROPIC_API_KEY`), pass it as a Docker secret or env var in `.env` (gitignored)

### 10.2 Claude Code Permissions

Workers run `claude -p` with `--dangerously-skip-permissions`, which means Claude Code can:
- Read/write any file the worker process can access
- Execute any command the worker user can run
- Make network requests

**Mitigation**:
- Worker runs as non-root user `claw`
- Mount only necessary directories
- Use `allowed_tools` per-job to restrict what Claude can do
- Consider using the `--allowed-tools` flag to whitelist specific tools

### 10.3 Network Security

- Redis should not be exposed to the public internet
- In production, use Redis AUTH (`redis://:password@host:6379`)
- API endpoints should be behind a reverse proxy with TLS
- Webhook endpoints should validate signatures (GitHub `X-Hub-Signature-256`)

### 10.4 Container Isolation

Each service runs in its own container with:
- Non-root user
- Read-only root filesystem (where possible)
- No privileged mode
- Limited volume mounts
