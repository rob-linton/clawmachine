# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

ClaudeCodeClaw is a job queue orchestrator for Claude Code. It wraps `claude -p` in a structured system: jobs are submitted via CLI/API/webhooks/cron/file-drops, parallel workers claim and execute them, and results flow back through Redis to a Flutter dashboard.

**Stack**: Rust (Axum) backend, Flutter (Riverpod) frontend, Redis (queue + state + streams), Docker Compose deployment.

**Auth**: Claude Code uses OAuth — workers inherit the host user's logged-in session (`~/.claude/`). No API key needed.

## Current State

The project has a working multi-crate Rust backend (6 crates), Flutter web dashboard, Redis queue, parallel workers, scheduler (cron + file watcher), and workspace management. Jobs execute in isolated workspaces with CLAUDE.md and `.claude/skills/` deployed on disk.

## Build & Run

```bash
cargo build --workspace                # Build all crates
cargo check                            # Type-check without building
cargo clippy -- -D warnings            # Lint
cargo test --workspace -- --test-threads=1  # Run tests (needs Redis on DB 15)

# Individual binaries:
cargo run -p claw-api                  # API server (port 8080)
cargo run -p claw-worker               # Worker (claims + executes jobs)
cargo run -p claw-scheduler            # Scheduler (cron + file watcher)
cargo run -p claw-cli                  # CLI tool
```

## Scripts

**Development (two terminals):**
```bash
# Terminal 1: Backend (API + Worker + Scheduler)
./scripts/backend.sh                   # Build + start all backend services (live output)
./scripts/backend.sh stop              # Stop all backend services

# Terminal 2: Frontend (Flutter hot reload)
./scripts/frontend.sh                  # Flutter dev server on :3000 with hot reload
./scripts/frontend.sh build            # Build release version for production
```

**Other scripts:**
```bash
./scripts/startup.sh                   # All-in-one: build + start everything + open browser
./scripts/startup.sh stop              # Stop all services
./scripts/submit.sh "your prompt"      # Submit a job to Redis
./scripts/result.sh <job_id>           # Check job status and result
```

**Log files:** All output is tee'd to `.logs/`:
- `.logs/api.log`, `.logs/worker.log`, `.logs/scheduler.log` — backend service logs
- `.logs/flutter-dev.log` — Flutter dev server output
- `.logs/build-rust.log`, `.logs/build-flutter.log` — build output

**Frontend config:** `flutter_ui/.env.dev` sets `API_URL` for development.

## Prerequisites

- Redis running locally (`redis-server` or `docker run -d -p 6379:6379 redis:7-alpine`)
- Claude Code CLI installed and authenticated via OAuth (`claude --version`)
- Rust toolchain (`rustc 1.83+`)
- Flutter SDK (`flutter 3.x+`) for the admin console

## Architecture

Full architecture docs live in `Documents/architecture/` (12 documents). Key references:

| Topic | Document |
|-------|----------|
| System overview & decisions | `00-overview.md` |
| System architecture | `01-system-architecture.md` |
| Redis schema, Lua scripts, job state machine | `02-data-model.md` |
| REST API + WebSocket spec | `03-api-specification.md` |
| Worker subprocess management | `04-worker-engine.md` |
| Skill injection mechanics | `05-skills-system.md` |
| CLI reference | `06-cli-reference.md` |
| Flutter UI design | `07-flutter-ui.md` |
| Docker deployment | `08-deployment.md` |
| Security & reliability | `09-security-and-reliability.md` |
| Implementation phases & self-testing | `10-implementation-roadmap.md` |

### Architecture

```
CLI → Axum API → Redis ← Workers (claude -p in workspace)
                   ↑              ↓
              Scheduler     Workspaces (CLAUDE.md + .claude/skills/)
                   ↓
            Flutter UI (http://localhost:8080)
```

**Crate structure**: `claw-models` → `claw-redis` → `claw-api`, `claw-worker`, `claw-scheduler`, `claw-cli`

### Key Design Decisions

- **CLI goes through API** (not direct Redis) — single codepath for validation/events/auth
- **Workspaces are first-class entities** — persistent directories with CLAUDE.md and skills. Jobs reference workspaces by ID. Redis is source of truth for CLAUDE.md; disk is written at job time.
- **Workspace locking** — one job at a time per persistent workspace (SETNX with TTL). Re-queue on contention. Temp workspaces don't need locks.
- **Atomic Lua scripts** for job claiming and workspace lock release — prevents races between parallel workers
- **Skill snapshotting** — `skill_snapshot` + `assembled_prompt` stored per-job for reproducibility
- **Skills deployed to disk** — Script skills written to `.claude/skills/{id}/SKILL.md` + bundled files. ClaudeConfig skills merged into workspace CLAUDE.md. Only Template skills injected into prompt text. Claude Code discovers disk skills natively.
- **Post-execution harvesting** — new skills created by Claude in `.claude/skills/` are captured back to Redis. Pre-existing skills are snapshotted before execution to avoid false positives.
- **CLAUDE.md crash recovery** — backup + marker files so worker cleanup survives unclean shutdown

## Flutter Semantics Rule

All Flutter widgets that display meaningful text (headings, names, labels, status indicators) **must** be wrapped with `Semantics` widgets. This is required for Playwright E2E testing since Flutter web renders to canvas and text is not in the DOM.

```dart
// ALWAYS do this for headings, list items, status text, etc.
Semantics(header: true, label: 'Page Title', child: Text('Page Title', ...))
Semantics(label: 'Skill ${skill.name}', child: Text(skill.name, ...))
Semantics(label: 'Connected', child: Text('Connected to ...', ...))
```

Without `Semantics`, Playwright tests cannot find or verify text content.

## Pipeline Endpoints

```
POST   /api/v1/pipelines              — create pipeline template (name, steps with prompts)
GET    /api/v1/pipelines              — list all pipelines
GET    /api/v1/pipelines/{id}         — get pipeline details
DELETE /api/v1/pipelines/{id}         — delete pipeline
POST   /api/v1/pipelines/{id}/run     — trigger pipeline run (submits first step as job)
GET    /api/v1/pipeline-runs          — list all pipeline runs
GET    /api/v1/pipeline-runs/{id}     — get run status + step job IDs
```

Steps can use `{{previous_result}}` placeholder to inject the previous step's output.

## Real-Time Events Endpoint

```
GET    /api/v1/events/jobs             — SSE stream of job status updates (via Redis Pub/Sub)
```

Clients receive `job_update` events with `{type, job_id, status}` payloads. The connection auto-sends keepalive pings.

## Authentication Endpoints

```
POST   /api/v1/auth/login              — login with {username, password}, returns session cookie
POST   /api/v1/auth/logout             — logout, clears session cookie
GET    /api/v1/auth/me                 — get current user {username, role}
POST   /api/v1/auth/users              — create user (admin only): {username, password, role}
GET    /api/v1/auth/users              — list users (admin only)
DELETE /api/v1/auth/users/{username}   — delete user (admin only)
```

Auth uses session cookies (`claw_session`) for the UI and bearer tokens (`CLAW_API_TOKEN`) for CLI/automation. Sessions are stored in Redis with 24h TTL. On first startup, an admin user is bootstrapped from `CLAW_ADMIN_USER`/`CLAW_ADMIN_PASSWORD` env vars (only if no users exist). Redis keys: `claw:user:{username}` (hash: password_hash, role, created_at), `claw:session:{uuid}` (hash: username, created_at, TTL 24h).

## Workspace File Endpoints

```
GET    /api/v1/workspaces/{id}/files              — list all files (up to depth 10, max 2000 entries, .git excluded)
GET    /api/v1/workspaces/{id}/files/{*path}      — read file content as text
PUT    /api/v1/workspaces/{id}/files/{*path}      — write file (body: {content: string})
DELETE /api/v1/workspaces/{id}/files/{*path}      — delete file or folder (recursive for dirs)
```

All file paths are validated server-side to prevent path traversal. Deleting a folder removes it and all contents recursively.

## Workspace History & Fork Endpoints

```
GET    /api/v1/workspaces/{id}/history        — git log (last 20 commits)
POST   /api/v1/workspaces/{id}/revert/{hash}  — git revert a specific commit
POST   /api/v1/workspaces/{id}/promote        — move claw/base tag (snapshot mode, query: ref=...)
POST   /api/v1/workspaces/{id}/sync           — pull latest from remote URL into bare repo
POST   /api/v1/workspaces/{id}/fork           — create new workspace from existing one (VMware-style fork)
GET    /api/v1/workspaces/{id}/branches       — list git branches (useful for snapshot workspaces)
GET    /api/v1/workspaces/{id}/events         — workspace event timeline (paginated: ?limit=50&offset=0)
```

Workspaces auto-commit before/after each job for rollback safety. Workspace events (initialized, forked, job started/completed/failed, file modified, etc.) are recorded for a human-readable history timeline.

## Workspace Persistence Modes

Workspaces support three persistence modes (set at creation, immutable after):

- **ephemeral** — Fresh clone each job. Claude's changes are discarded. Base state is maintained in the bare repo and editable via the file browser.
- **persistent** — Changes accumulate across jobs. Full git history. Post-job commits are pushed back to the bare repo.
- **snapshot** — Fresh clone from a `claw/base` tag each job. Results pushed to snapshot branches for inspection. Use the promote endpoint to update the base tag.

New workspaces use git bare repos at `~/.claw/repos/{id}.git` with working checkouts at `~/.claw/checkouts/{id}/` for the file browser. Legacy workspaces with explicit `path` field continue to work unchanged.

## System Config Endpoints

```
GET    /api/v1/config                — get all system config as JSON
PUT    /api/v1/config                — update config (partial merge)
GET    /api/v1/config/{key}          — get single config value
PUT    /api/v1/config/{key}          — set single config value
```

Config stored in Redis (`claw:config:*` keys) with sane defaults. Editable from the Settings screen.

## Docker Management Endpoints

```
GET    /api/v1/docker/status         — Docker daemon availability + info
GET    /api/v1/docker/images         — list sandbox images
POST   /api/v1/docker/images/pull    — pull sandbox image
POST   /api/v1/docker/images/build   — build sandbox from bundled Dockerfile
```

## Execution Backend

Jobs can execute locally (direct `claude -p` subprocess) or inside Docker containers. Controlled via `execution_backend` config key (`local` or `docker`). The worker re-reads this config before each job claim — changes from Settings take effect without worker restart.

Docker execution uses a sandbox image (`claw-sandbox:latest` by default) with Claude Code + gh CLI pre-installed. Containers run with `--user` matching the host UID/GID. Resource limits (memory, CPU, PIDs) configurable globally and per-workspace.

## Job Template Endpoints

```
POST   /api/v1/job-templates              — create reusable job template
GET    /api/v1/job-templates              — list all templates
GET    /api/v1/job-templates/{id}         — get template details
PUT    /api/v1/job-templates/{id}         — update template
DELETE /api/v1/job-templates/{id}         — delete (409 if referenced by crons/pipelines)
POST   /api/v1/job-templates/{id}/run     — run template immediately as a new job
```

Templates are reusable job definitions (prompt, skills, workspace, model, etc.) that crons and pipeline steps can reference via `template_id`.

## Upload Endpoints

ZIP file upload for bulk importing files into workspaces and skills:

```
POST /api/v1/workspaces/{id}/upload   — multipart: file=<zip>, [path=<prefix>]
POST /api/v1/skills/upload            — multipart: file=<zip>, id, name, skill_type, [description], [tags]
```

Both endpoints auto-strip common root directory prefixes from zip entries (e.g. `my-skill/SKILL.md` → `SKILL.md`). Limits: 100MB zip, 10MB per file, 5000 max entries, zip bomb protection via cumulative size tracking.

## Self-Testing Rule

Every phase must be validated end-to-end before proceeding. After writing code, exercise it as a real user: hit the API with curl, submit jobs via CLI, open the UI in a browser. See `10-implementation-roadmap.md` section 10 for the full testing protocol.

## Environment Variables

| Variable | Default | Purpose |
|----------|---------|---------|
| `CLAW_REDIS_URL` | `redis://127.0.0.1:6379` | Redis connection |
| `CLAW_API_URL` | `http://127.0.0.1:8080` | API server URL (for CLI) |
| `CLAW_API_PORT` | `8080` | API server listen port |
| `CLAW_STATIC_DIR` | `flutter_ui/build/web` | Flutter build directory to serve |
| `CLAW_WORKER_CONCURRENCY` | `1` | Number of parallel worker tasks |
| `CLAW_LOG_FORMAT` | (text) | Set to `json` for structured JSON logging |
| `CLAW_API_TOKEN` | (unset) | API bearer token for CLI/automation. If set, acts as admin-level auth |
| `CLAW_ADMIN_USER` | (unset) | Bootstrap admin username (only used when no users exist) |
| `CLAW_ADMIN_PASSWORD` | (unset) | Bootstrap admin password (only used when no users exist) |
| `CLAW_CORS_ORIGIN` | (permissive) | Restrict CORS to this origin (e.g., `https://192.168.1.50`) |
| `CLAW_REDIS_PASSWORD` | (unset) | Redis AUTH password (used in docker-compose) |
| `CLAW_HOST_IP` | `localhost` | Server IP for Caddy TLS cert and CORS |
| `CLAW_FAILURE_WEBHOOK_URL` | (unset) | POST to this URL when a job fails |
| `CLAW_COMPLETION_WEBHOOK_URL` | (unset) | POST to this URL when any job completes |
| `CLAW_WORKSPACES_DIR` | `~/.claw/workspaces` | Base directory for legacy workspaces |
| `CLAW_EXECUTION_BACKEND` | `docker` | Fallback if Redis config not set: `local` or `docker`. Redis default is `local` for dev safety |
| `CLAW_DATA_DIR` | `/opt/claw/data` | Host path for workspace data bind mount |
| `CLAW_HOST_DATA_DIR` | `/opt/claw/data` | Host path for Docker-in-Docker volume mapping (set inside worker container) |
| `CLAW_HOST_CLAUDE_HOME` | `~/.claude` | Host path for Claude credentials (Docker-in-Docker volume mapping) |

Most new configuration is stored in Redis (`claw:config:*`) and managed from the Settings screen. Env vars are only used as bootstrap fallbacks.

## Docker Isolation

Jobs execute inside sandbox containers (`claw-sandbox:latest`) by default. Each job gets its own container with per-workspace resource limits. The worker spawns sandbox containers via the Docker socket.

**Defaults**: execution backend `docker`, network mode `bridge` (Claude Code requires API access), memory `4g`, CPU `2.0`, PIDs `256`.

**Per-workspace overrides**: `base_image`, `memory_limit`, `cpu_limit`, `network_mode` on the workspace override global Docker config.

**Docker-in-Docker**: When the worker runs in a container, job dirs are at `~/.claw/jobs/{id}` (inside the shared bind mount). `CLAW_HOST_DATA_DIR` maps container paths to host paths for sandbox container volume mounts.
