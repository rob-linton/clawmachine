# ClaudeCodeClaw

A job queue orchestrator for [Claude Code](https://claude.ai/code). Submit prompts via CLI, API, webhooks, cron schedules, or file drops. Parallel workers claim and execute them using `claude -p` in isolated workspaces. Results flow back through Redis to a Flutter web dashboard.

```
CLI / API / Cron / File Drop
        |
   Axum REST API ──> Redis Queue ──> Workers (claude -p)
        |                                |
   Flutter Dashboard            Workspaces (git-backed)
```

## Quick Start

### Prerequisites

| Tool | Version | Install |
|------|---------|---------|
| Rust | 1.83+ | [rustup.rs](https://rustup.rs) |
| Flutter | 3.x+ | [flutter.dev](https://flutter.dev/docs/get-started/install) |
| Redis | 7+ | `docker run -d -p 6379:6379 redis:7-alpine` |
| Claude Code | Latest | `npm install -g @anthropic-ai/claude-code` |
| Docker | 20.10+ | Optional, for sandboxed execution |

**Authenticate Claude Code** before starting (workers inherit your session):

```bash
claude   # Complete OAuth login once
```

### Start Everything

```bash
git clone <repo-url> && cd claudecodeclaw

# One command — builds, starts all services, opens browser
./scripts/startup.sh
```

Dashboard opens at **http://localhost:8080**.

### Submit Your First Job

```bash
# Via script
./scripts/submit.sh "What is the capital of France?"
./scripts/result.sh <job_id>

# Via API
curl -X POST http://localhost:8080/api/v1/jobs \
  -H "Content-Type: application/json" \
  -d '{"prompt": "What is the capital of France?"}'

# Via dashboard
# Click "New Job" in the UI
```

## Development Setup

Use two terminals for the best dev experience:

**Terminal 1 — Backend** (API + Worker + Scheduler):
```bash
./scripts/backend.sh           # Live output, auto-restarts on crash
./scripts/backend.sh stop      # Stop all services
```

**Terminal 2 — Frontend** (Flutter hot reload):
```bash
./scripts/frontend.sh          # Dev server on :3000 with hot reload
./scripts/frontend.sh build    # Build release for production
```

Logs are written to `.logs/api.log`, `.logs/worker.log`, `.logs/scheduler.log`.

### Build Commands

```bash
cargo build --workspace                        # Build all crates
cargo check                                    # Type-check only
cargo clippy -- -D warnings                    # Lint
cargo test --workspace -- --test-threads=1     # Tests (needs Redis)
```

## Architecture

### Crate Structure

```
claw-models  ──>  claw-redis  ──>  claw-api
                      |            claw-worker
                      |            claw-scheduler
                      |            claw-cli
```

- **claw-models** — Shared types (Job, Workspace, Skill, Pipeline)
- **claw-redis** — Redis client with atomic Lua scripts for job claiming
- **claw-api** — Axum REST API + Flutter static file serving
- **claw-worker** — Claims jobs, runs `claude -p`, streams results
- **claw-scheduler** — Cron engine + file watcher for job submission
- **claw-cli** — Command-line interface

### How Jobs Execute

1. Job submitted via API → stored in Redis → pushed to priority queue
2. Worker claims job atomically (Lua script prevents double-claiming)
3. Worker resolves workspace (clone git repo or use legacy directory)
4. Skills + CLAUDE.md deployed to workspace
5. `claude -p "<prompt>" --output-format stream-json` runs in workspace
6. Output streamed to Redis (real-time log forwarding)
7. Result stored, workspace cleaned up, next job claimed

### Workspaces

Workspaces are isolated environments where Claude executes. Three persistence modes:

| Mode | Behavior | Use Case |
|------|----------|----------|
| **Persistent** | Changes accumulate across jobs. Full git history. | Active projects |
| **Ephemeral** | Fresh clone each job. Changes discarded. | CI tasks, one-off jobs |
| **Snapshot** | Clone from a tagged base. Optionally promote results. | Reproducible builds |

New workspaces are backed by git bare repos at `~/.claw/repos/{id}.git` with working checkouts at `~/.claw/checkouts/{id}/` for the file browser. Legacy workspaces with explicit paths continue to work.

Create workspaces from the dashboard — set persistence mode, optional remote URL (GitHub/Gitea), skills, and CLAUDE.md.

### Execution Backends

Jobs can run in two modes, switchable from **Settings** in the dashboard:

**Local** (default) — `claude -p` runs directly on the host. Simple, uses host OAuth tokens.

**Docker** (sandboxed) — Each job runs in an isolated container with Claude Code + gh CLI pre-installed. Resource limits (memory, CPU, PIDs) configurable per-workspace. Build or pull the sandbox image from the Settings screen — no terminal commands needed.

## API Reference

### Jobs
```
POST   /api/v1/jobs                    — submit job
GET    /api/v1/jobs                    — list jobs (?status=pending&limit=20)
GET    /api/v1/jobs/{id}               — get job details
GET    /api/v1/jobs/{id}/result        — get result
GET    /api/v1/jobs/{id}/logs          — get log lines
POST   /api/v1/jobs/{id}/cancel        — cancel running job
DELETE /api/v1/jobs/{id}               — delete job
```

### Workspaces
```
POST   /api/v1/workspaces             — create workspace
GET    /api/v1/workspaces             — list workspaces
GET    /api/v1/workspaces/{id}        — get workspace
PUT    /api/v1/workspaces/{id}        — update workspace
DELETE /api/v1/workspaces/{id}        — delete (?delete_files=true)
GET    /api/v1/workspaces/{id}/files  — list files
GET    /api/v1/workspaces/{id}/files/{path} — read file
PUT    /api/v1/workspaces/{id}/files/{path} — write file
DELETE /api/v1/workspaces/{id}/files/{path} — delete file
POST   /api/v1/workspaces/{id}/upload — upload ZIP
GET    /api/v1/workspaces/{id}/history — git log
POST   /api/v1/workspaces/{id}/revert/{hash} — git revert
POST   /api/v1/workspaces/{id}/promote — move snapshot base tag (?ref=...)
POST   /api/v1/workspaces/{id}/sync   — pull from remote URL
```

### Skills
```
POST   /api/v1/skills                 — create skill
GET    /api/v1/skills                 — list skills
GET    /api/v1/skills/{id}            — get skill
PUT    /api/v1/skills/{id}            — update skill
DELETE /api/v1/skills/{id}            — delete skill
POST   /api/v1/skills/upload          — upload skill ZIP
```

### Pipelines
```
POST   /api/v1/pipelines              — create pipeline
GET    /api/v1/pipelines              — list pipelines
DELETE /api/v1/pipelines/{id}         — delete pipeline
POST   /api/v1/pipelines/{id}/run     — run pipeline
GET    /api/v1/pipeline-runs          — list runs
GET    /api/v1/pipeline-runs/{id}     — get run status
```

### Schedules (Crons)
```
POST   /api/v1/crons                  — create schedule
GET    /api/v1/crons                  — list schedules
PUT    /api/v1/crons/{id}             — update schedule
DELETE /api/v1/crons/{id}             — delete schedule
POST   /api/v1/crons/{id}/trigger     — trigger now
```

### Job Templates
```
POST   /api/v1/job-templates          — create template
GET    /api/v1/job-templates          — list templates
PUT    /api/v1/job-templates/{id}     — update template
DELETE /api/v1/job-templates/{id}     — delete template
POST   /api/v1/job-templates/{id}/run — run template
```

### System
```
GET    /api/v1/status                 — health + queue + docker + worker info
GET    /api/v1/config                 — get all system config
PUT    /api/v1/config                 — update config (partial merge)
GET    /api/v1/docker/status          — Docker availability
GET    /api/v1/docker/images          — list sandbox images
POST   /api/v1/docker/images/pull     — pull sandbox image
POST   /api/v1/docker/images/build    — build sandbox image
GET    /api/v1/events/jobs            — SSE stream of job updates
```

## Configuration

### Environment Variables

| Variable | Default | Purpose |
|----------|---------|---------|
| `CLAW_REDIS_URL` | `redis://127.0.0.1:6379` | Redis connection |
| `CLAW_API_PORT` | `8080` | API server port |
| `CLAW_API_TOKEN` | (unset) | If set, requires `Authorization: Bearer <token>` |
| `CLAW_WORKER_CONCURRENCY` | `1` | Parallel jobs per worker |
| `CLAW_EXECUTION_BACKEND` | `local` | `local` or `docker` (fallback if Redis config not set) |
| `CLAW_STATIC_DIR` | `flutter_ui/build/web` | Flutter build directory |
| `CLAW_FAILURE_WEBHOOK_URL` | (unset) | POST on job failure |
| `CLAW_COMPLETION_WEBHOOK_URL` | (unset) | POST on any job completion |
| `RUST_LOG` | `info` | Log level (`debug`, `info`, `warn`) |

Most new configuration is stored in Redis and managed from the **Settings** screen in the dashboard.

### Settings Screen

The dashboard Settings page lets you configure everything without touching env vars:

- **Execution Backend** — Toggle between Local and Docker
- **Sandbox Image** — Pull or build the Docker sandbox image
- **Resource Limits** — Default memory, CPU, PID limits for Docker containers
- **Credential Mounts** — Host paths mounted into containers (Claude OAuth, gh CLI, SSH keys)
- **System Health** — Docker status, Redis connection, worker count

## Docker Deployment

### Docker Compose

```bash
# Build and start (Redis + API + Scheduler)
docker compose -f docker/docker-compose.yml up --build -d

# Worker runs on the host (not in container — avoids Docker-in-Docker)
cargo run -p claw-worker

# Scale workers
CLAW_WORKER_CONCURRENCY=3 cargo run -p claw-worker
```

The worker runs on the host because it needs to create Docker containers for sandboxed execution. Running a worker inside Docker would require Docker-in-Docker, which adds complexity and has path mapping issues.

### Sandbox Image

The sandbox image contains Claude Code + common tools for job execution:

```bash
# Build from Settings screen (recommended)
# Or manually:
docker build -f docker/Dockerfile.sandbox -t claw-sandbox:latest docker/
```

Contents: Debian slim, git, Node.js, Claude Code CLI, GitHub CLI.

## Testing

### Unit Tests
```bash
cargo test --workspace -- --test-threads=1   # Needs Redis on test DB
```

### E2E Tests (Playwright)
```bash
./scripts/startup.sh                          # Start everything first

cd tests/e2e
npm install
npx playwright test                           # Run all E2E tests
npx playwright test --reporter=list           # Verbose output
npx playwright test specs/workspace-rearchitect.spec.ts  # Specific suite
```

### Manual Smoke Test
```bash
./scripts/submit.sh "Say hello"
./scripts/result.sh <job_id>
curl http://localhost:8080/api/v1/status
```

## Project Structure

```
claudecodeclaw/
├── crates/                    # Rust workspace (6 crates)
│   ├── claw-models/           # Shared types
│   ├── claw-redis/            # Redis client + config
│   ├── claw-api/              # REST API server
│   ├── claw-worker/           # Job executor
│   ├── claw-scheduler/        # Cron + file watcher
│   └── claw-cli/              # CLI tool
├── flutter_ui/                # Flutter web dashboard
├── docker/                    # Dockerfiles + compose
│   ├── Dockerfile.backend     # Multi-stage (API, worker, scheduler)
│   ├── Dockerfile.sandbox     # Sandbox image for Docker execution
│   └── docker-compose.yml     # Production deployment
├── scripts/                   # Dev scripts (startup, submit, result)
├── tests/e2e/                 # Playwright E2E tests
├── Documents/architecture/    # 12 architecture documents
├── CLAUDE.md                  # Project instructions for Claude Code
└── .logs/                     # Runtime logs
```

## License

MIT
