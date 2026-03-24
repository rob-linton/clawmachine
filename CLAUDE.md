# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

Claw Machine is a job queue orchestrator for Claude Code. It wraps `claude -p` in a structured system: jobs are submitted via CLI/API/webhooks/cron/file-drops, parallel workers claim and execute them, and results flow back through Redis to a Flutter dashboard.

**Stack**: Rust (Axum) backend, Flutter (Riverpod) frontend, Redis (queue + state + streams), Docker Compose deployment.

**Auth**: Claude Code uses OAuth — workers inherit the host user's logged-in session (`~/.claude/`). No API key needed. See "Claude Code OAuth Token Lifecycle" section for critical token refresh requirements.

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
PUT    /api/v1/pipelines/{id}         — update pipeline (same body as create)
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
GET    /api/v1/workspaces/{id}/files/{*path}      — read file content as JSON {path, content}
GET    /api/v1/workspaces/{id}/files/{*path}?raw=true    — serve raw file bytes (inline, correct Content-Type)
GET    /api/v1/workspaces/{id}/files/{*path}?download=true — download file (Content-Disposition: attachment)
PUT    /api/v1/workspaces/{id}/files/{*path}      — write file (body: {content: string})
DELETE /api/v1/workspaces/{id}/files/{*path}      — delete file or folder (recursive for dirs)
GET    /api/v1/workspaces/{id}/download           — download entire workspace as ZIP (excludes .git/.claw, 500MB limit)
GET    /api/v1/workspaces/{id}/download?path=dir/ — download subdirectory as ZIP
```

All file paths are validated server-side to prevent path traversal. Deleting a folder removes it and all contents recursively. Raw/download mode serves binary files with MIME types based on file extension.

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

## Tool Provisioning Endpoints

```
POST   /api/v1/tools                        — create CLI tool definition
GET    /api/v1/tools                        — list all tools
GET    /api/v1/tools/{id}                   — get tool details
PUT    /api/v1/tools/{id}                   — update tool
DELETE /api/v1/tools/{id}                   — delete tool
POST   /api/v1/tools/install-from-url       — install tool from git repo or ZIP URL
POST   /api/v1/tools/{id}/update-from-source — re-fetch tool from source_url
POST   /api/v1/skills/install-from-url      — install skill from git repo or ZIP URL
POST   /api/v1/skills/{id}/update-from-source — re-fetch skill from source_url
```

Skills and tools have an `enabled` field (default true). Disabled items are filtered out by the worker and not deployed to jobs. Install from URL supports git repos and direct ZIP downloads with SSRF protection (HTTPS only). The `source_url` field tracks where a skill/tool was installed from, enabling updates via `update-from-source`.

**Curated catalog**: Set `catalog_url` in Settings (Redis config key `claw:config:catalog_url`) to a JSON catalog URL. The Skills/Tools screens show a "Recommended" section with one-click install from the catalog.

Tools are CLI programs (az, aws, gh, etc.) installed into Docker sandbox images on demand. Each tool defines `install_commands` (Debian shell), `check_command` (verify presence), optional `auth_script` (login before job), `env_vars` (credentials needed), and optional `skill_content` (usage guide). Jobs, templates, workspaces, crons, and pipeline steps can reference tools via `tool_ids`.

**Usage guides**: Tools with `skill_content` get a SKILL.md deployed to `.claude/skills/tool-{id}/` in the workspace during job preparation. This gives Claude Code native instructions on how to use the tool. The skill is cleaned up after job execution.

**Docker mode**: Tools are baked into derived images (`claw-tools:{hash}`) cached by content hash. First job builds the image; subsequent jobs reuse it instantly.

**Local mode**: Tools are check-only (`check_command`). The worker verifies the tool is present but does not attempt installation.

**Auth scripts**: When tools have `auth_script`, the Docker entrypoint is overridden to run auth commands before `claude -p`. The prompt is written to a script file to prevent shell injection.

## OAuth Login Endpoints

```
GET    /api/v1/auth/oauth-status      — current OAuth token status (valid/expired/missing + expiry)
```

**Two authentication methods** (OAuth preferred when available):

1. **OAuth (subscription)**: Run `claude auth login` interactively on the server host. The worker auto-refreshes the token every ~24h. Uses the Claude subscription (Max plan) — no per-API-call billing.

2. **API Key (billed)**: Set `ANTHROPIC_API_KEY` via Settings UI or `.env`. Used as fallback when OAuth is unavailable. Billed per API call.

**Auth preference order**: OAuth > API key. When valid OAuth tokens exist, the worker does NOT pass `ANTHROPIC_API_KEY` to Claude Code processes.

## Credential Endpoints

```
POST   /api/v1/credentials           — create credential (with encrypted values)
GET    /api/v1/credentials           — list credentials (values masked as "***set***")
GET    /api/v1/credentials/{id}      — get credential metadata (values masked)
PUT    /api/v1/credentials/{id}      — update credential values
DELETE /api/v1/credentials/{id}      — delete credential
```

Credentials store encrypted key-value pairs (e.g., `AWS_ACCESS_KEY_ID`, `AZURE_CLIENT_SECRET`) used by tool auth scripts. Encryption uses AES-256-GCM with the key from `CLAW_SECRET_KEY` env var. Values are never returned in API responses. Credentials are bound to tools via `credential_bindings` on the workspace (maps tool_id → credential_id). At job time, the worker decrypts bound credentials and injects them as container env vars.

## Upload Endpoints

ZIP file upload/download for workspaces, skills, and tools:

```
POST /api/v1/workspaces/{id}/upload   — multipart: file=<zip>, [path=<prefix>]
POST /api/v1/skills/upload            — multipart: file=<zip>, id, name, [description], [tags]
GET  /api/v1/skills/{id}/download     — export skill as ZIP (SKILL.md + manifest.json + bundled files)
POST /api/v1/tools/upload             — multipart: file=<zip>, id, name, [description], [tags]
GET  /api/v1/tools/{id}/download      — export tool as ZIP (TOOL.json + manifest.json)
```

Upload endpoints auto-strip common root directory prefixes from zip entries (e.g. `my-skill/SKILL.md` → `SKILL.md`). Limits: 100MB zip, 10MB per file, 5000 max entries, zip bomb protection via cumulative size tracking.

Skill and tool ZIPs include a `manifest.json` with package metadata (format, id, name, version, author, license, description, tags). On import, manifest values auto-populate; form fields override. See `Documents/SKILL-FORMAT.md` and `Documents/TOOL-FORMAT.md` for full package specifications.

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
| `CLAW_DATA_DIR` | `~/.claw-data` | Host path for workspace data bind mount |
| `CLAW_HOST_DATA_DIR` | `~/.claw-data` | Host path for Docker-in-Docker volume mapping (set automatically from CLAW_DATA_DIR) |
| `CLAW_HOST_CLAUDE_HOME` | `~/.claude` | Host path for Claude credentials (Docker-in-Docker volume mapping) |
| `CLAW_SECRET_KEY` | (unset) | Encryption key for tool credentials (32-byte hex, base64, or passphrase). Required for credential storage |
| `ANTHROPIC_API_KEY` | (unset) | Anthropic API key. If set, bypasses OAuth entirely. Also settable via Settings UI (Redis config `anthropic_api_key` takes priority over env var) |

Most new configuration is stored in Redis (`claw:config:*`) and managed from the Settings screen. Env vars are only used as bootstrap fallbacks.

## Docker Isolation

Jobs execute inside sandbox containers (`claw-sandbox:latest`) by default. Each job gets its own container with per-workspace resource limits. The worker spawns sandbox containers via the Docker socket.

**Defaults**: execution backend `docker`, network mode `bridge` (Claude Code requires API access), memory `4g`, CPU `2.0`, PIDs `256`, budget `$1000`, timeout `30 minutes`.

**Per-workspace overrides**: `base_image`, `memory_limit`, `cpu_limit`, `network_mode` on the workspace override global Docker config.

**Docker-in-Docker**: When the worker runs in a container, job dirs are at `~/.claw/jobs/{id}` (inside the shared bind mount). `CLAW_HOST_DATA_DIR` maps container paths to host paths for sandbox container volume mounts.

### Sandbox Container Requirements (critical)

These requirements were discovered through production testing and must not be regressed:

1. **`HOME=/home/claw`** must be set via `-e HOME=/home/claw` on `docker run`. Without it, Claude Code cannot find `~/.claude/` and `~/.claude.json`, producing zero stdout and appearing hung.

2. **`~/.claude.json` must be mounted read-write** (not `:ro`). Claude Code writes to this file at runtime. Read-only mount causes `EROFS` errors and early exit after ~7 turns.

3. **`~/.claude/` must be mounted read-write**. Claude Code writes session state to `~/.claude/session-env/` at runtime.

4. **Node.js 20+** required in the sandbox image. Debian bookworm ships Node 18 which lacks `Array.with()` (added in Node 20). Claude Code crashes with `TypeError: A.with is not a function` on Node 18.

5. **Workspace files must be owned by the sandbox user** (not root). The worker runs as root but the sandbox runs as the `.claude/` owner (typically uid 1000). After cloning a workspace, the worker `chown -R` the job dir to match.

6. **Worker runs as root** for Docker socket access. The sandbox container runs as the authenticated user (uid from `~/.claude/` ownership). `--dangerously-skip-permissions` is rejected when running as root.

7. **`git config --global safe.directory '*'`** required in worker image. Worker is root but bare repos are owned by claw user — git refuses operations without this.

### Claude Code OAuth Token Lifecycle (critical)

Claude Code authenticates via OAuth with two tokens stored in `~/.claude/.credentials.json` under the `claudeAiOauth` key:

- **Access token** (`accessToken`): Short-lived (~24h). Sent as bearer token to the Anthropic API. Stored with `expiresAt` (epoch ms).
- **Refresh token** (`refreshToken`): Long-lived (~3 months). Used to obtain new access tokens. **Single-use** — each refresh returns a new refresh token that must be saved.

**Token refresh endpoint**: `POST https://console.anthropic.com/v1/oauth/token` with `Content-Type: application/json`:
```json
{"grant_type": "refresh_token", "refresh_token": "<token>", "client_id": "9d1c250a-e61b-44d9-88ed-5944d1962f5e"}
```
Returns `{access_token, refresh_token, expires_in, token_type}`. The new `refresh_token` MUST be saved — the old one is invalidated.

**Critical issues discovered in production**:

1. **Claude Code `-p` mode does NOT auto-refresh expired tokens.** When the access token expires, `claude -p` returns a 401 error and exits with code 1 instead of using the refresh token. This means jobs will fail silently after ~24h if nothing refreshes the token.

2. **Refresh tokens are single-use.** If a refresh is attempted but the new tokens aren't saved (e.g., process crash mid-refresh), the refresh token is consumed and the credential file becomes permanently invalid. The user must re-authenticate interactively (`claude login`).

3. **The worker must proactively refresh tokens before they expire.** The worker (or a scheduled task) should check `expiresAt` and refresh the token when it's within ~1h of expiry. The refresh must be atomic: read credentials → call refresh endpoint → write new tokens in one operation with file locking.

## OAuth Redis Keys

```
claw:worker:oauth_status               — JSON: {status, expires_at, refresh_token_age_days, oauth_url?} (written by worker)
claw:oauth-login:active                — Lock key (SETNX with 600s TTL, prevents concurrent logins)
claw:oauth-login:request               — Pub/sub channel: login requests from API to worker
```

## Tool Redis Keys

```
claw:tool:{id}                     — JSON Tool object
claw:tools:index                   — Set of all tool IDs
claw:tool-image:{hash}             — Derived Docker image tracking (tag, tool_ids, timestamps)
claw:credential:{id}               — JSON Credential metadata (Phase 2)
claw:credential:{id}:values        — Encrypted credential values (Phase 2)
```

## Workspace Redis Keys

```
claw:workspace:{uuid}              — JSON workspace object
claw:workspaces:index              — Set of all workspace UUIDs
claw:workspace:{uuid}:lock         — Exclusive lock (job_id string, TTL)
claw:workspace:{uuid}:events       — List of JSON WorkspaceEvent entries (newest first, capped at 1000)
claw:workspace:{uuid}:children     — Set of child workspace UUIDs (for lineage tracking)
```

## Production Server

The server runs at `10.0.0.10`, accessible via `ssh claw-server` (user `developer`, key `~/.ssh/id_ed25519_claw_server`). Caddy reverse-proxies ports 80/443 to the API on :8080, so use `http://10.0.0.10` (not `:8080`) for API calls from the server.

**Deployment directory**: `~/claw/` on the server contains `docker-compose.yml`, `Caddyfile`, `.env`. This compose file is **generated by `scripts/install.sh`** — it uses pre-built images from ghcr.io, not the repo's `docker/docker-compose.yml` which builds from source.

**API authentication**: No `CLAW_API_TOKEN` is set. Use session auth — login via `/api/v1/auth/login` with credentials from `~/claw/.env` (`CLAW_ADMIN_USER`/`CLAW_ADMIN_PASSWORD`), then pass the `claw_session` cookie.

## Docker Images & Deployment

Images are hosted at `ghcr.io/rob-linton/clawmachine/{api,worker,scheduler,sandbox}`.

**Build and push workflow** (from project root on macOS, cross-compiles for linux/amd64):
```bash
# Login to ghcr.io (uses gh CLI token)
gh auth token | docker login ghcr.io -u rob-linton --password-stdin

# Build and push individual images — ALWAYS include --platform linux/amd64
docker buildx build --platform linux/amd64 --target api -t ghcr.io/rob-linton/clawmachine/api:latest --push -f docker/Dockerfile.backend .
docker buildx build --platform linux/amd64 --target scheduler -t ghcr.io/rob-linton/clawmachine/scheduler:latest --push -f docker/Dockerfile.backend .
docker buildx build --platform linux/amd64 -t ghcr.io/rob-linton/clawmachine/worker:latest --push -f docker/Dockerfile.worker .
docker buildx build --platform linux/amd64 -t ghcr.io/rob-linton/clawmachine/sandbox:latest --push -f docker/Dockerfile.sandbox .
```

**CRITICAL: `--platform linux/amd64` is mandatory on every build.** The dev machine is ARM (Apple Silicon) but the server is x86_64. Omitting the platform flag pushes an ARM image that crashes with `exec format error` on the server.

**Deploy to server** (after pushing images):
```bash
ssh claw-server "cd ~/claw && docker compose --env-file .env pull && docker compose --env-file .env up -d"
```

**UI changes require rebuilding the API image** because Flutter web is bundled into it via `Dockerfile.backend` (multi-stage: flutter-builder → rust-builder → api stage with static files). Run `flutter build web` locally first to verify, but the Docker build does its own Flutter build.

**`docker-compose.yml` must have `name: claw`** — this pins container names to `claw-redis-1`, `claw-api-1`, etc. regardless of the directory. Without it, Docker Compose derives the project name from the directory, causing orphaned containers/volumes if the install path changes.

## Sandbox Image

`Dockerfile.sandbox` builds the sandbox that jobs execute in. Base: Debian bookworm-slim + Node.js 20 + Claude Code CLI + gh CLI. Also includes common tools: `unzip`, `zip`, `jq`, `wget`, `gnupg`, `python3`, `pip`, `build-essential`. Tool install scripts (e.g., AWS CLI) can assume these are present.

After pushing a new sandbox image, pull and retag on the server:
```bash
ssh claw-server "docker pull ghcr.io/rob-linton/clawmachine/sandbox:latest && docker tag ghcr.io/rob-linton/clawmachine/sandbox:latest claw-sandbox:latest"
```

## Catalog (claw-catalog)

`claw-catalog/` is a **separate git repo** (`github.com/rob-linton/claw-catalog`), gitignored from the main repo. It contains curated skills and tools that auto-sync to Claw Machine instances.

**Structure**: `catalog.json` (index), `skills/{id}/` (SKILL.md + manifest.json), `tools/{id}/` (TOOL.json + manifest.json).

**Sync behavior**: The Skills/Tools screens trigger `POST /api/v1/catalog/sync` on every refresh. The API also auto-syncs 10 seconds after startup. Sync clones the catalog repo, compares versions, and installs/updates items. Items with matching IDs but no `source_url` (manually created) are skipped — delete them first to let the catalog version take over.

**Version bumps**: Catalog sync only updates items when the version in `catalog.json` differs from what's in Redis. Bump the version in both `catalog.json` and `manifest.json` when changing tool definitions.

**Tool images**: When a tool has `install_commands`, the worker builds a derived Docker image (`claw-tools:{hash}`) cached by content hash. Clear cached images with `docker rmi` on the server when changing install commands, otherwise the old image is reused.

## Template-First Architecture

Job Templates are the primary building block. Pipelines and Schedules reference templates rather than defining inline prompts/skills/tools:

- **Job Templates** define reusable job configurations (prompt, skills, tools, model, workspace, timeout)
- **Pipelines** compose templates into ordered steps. Each step references a template via `template_id` (optional for backward compatibility — inline prompts still work). Steps can use `{{previous_result}}` to chain outputs.
- **Schedules** reference a template via `template_id` for the recurring job definition. Inline prompt is supported as a fallback.
- Template deletion is protected: returns 409 if referenced by any pipeline step or schedule.
- The worker resolves `template_id` at job execution time, so template changes take effect on the next run without updating pipelines/schedules.

## Flutter UI Patterns

### Full-Page Create/Edit Screens

Templates, Pipelines, and Schedules use full-page create/edit screens (not dialogs). All follow the same pattern:

- `ConsumerStatefulWidget` with optional ID parameter (null = create, non-null = edit)
- Routes: `/entity/create` and `/entity/:id/edit` (static path before parameterized to avoid GoRouter matching "create" as an ID)
- `initState` → `_loadData()` fetches reference data (skills, tools, workspaces, templates) and populates controllers in edit mode
- Layout: header row (back button + title + save button) → `SingleChildScrollView` → `Center` → `ConstrainedBox(maxWidth: 900)` → `Column`
- Save calls create or update API method based on mode, then `context.go('/entity')` to return to list

### General Notes

- **SelectionArea** wraps the entire app (`MaterialApp.router` builder) so all text is selectable/copyable. No need for per-widget `SelectableText` or copy buttons.
- **`Image.network('clawmachine_logo.png')`** loads the logo from the web directory (served as a static asset). Use `filterQuality: FilterQuality.none` for pixel art to stay crisp.
- **Docker builds must run from the project root** — running from a subdirectory causes `lstat docker: no such file or directory`.
- The login screen, nav rail, and web manifest all reference the Claw Machine branding.

## Install Script

`scripts/install.sh` handles fresh server deployments per `INSTALL.md`. It generates its own `docker-compose.yml`, `.env`, and `Caddyfile` — these are separate from the dev versions in `docker/`. The script pulls pre-built images from ghcr.io. Default install path is `~/claw` (pass an argument to override). Do **not** re-run the install script on an existing deployment — it regenerates the Redis password and loses `CLAW_SECRET_KEY`.
