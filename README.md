# Claw Machine

A self-hosted job orchestrator for [Claude Code](https://claude.ai/code). Run AI coding tasks in isolated Docker containers with git-backed workspaces, pipelines, schedules, and a web dashboard.

```
CLI / API / Cron / Webhooks
        |
   Axum REST API ──> Redis Queue ──> Workers
        |                                |
   Flutter Dashboard         Docker Sandbox Containers
                                         |
                              Git-backed Workspaces
```

## Why Claw Machine?

- **Docker isolation** — Every job runs in its own sandbox container with memory, CPU, and network limits. No job can affect the host or other jobs.
- **Git-backed workspaces** — Workspaces are versioned with git. Fork workspaces, revert to any commit, create snapshots, and track full history.
- **Pipelines** — Chain jobs together. Each step passes context to the next. Results from step 1 become input for step 2.
- **Schedules** — Run jobs on cron schedules. Generate daily reports, run nightly analysis, poll APIs on intervals.
- **Real-time streaming** — Watch Claude work in real-time via the web dashboard. Logs stream as they're produced.
- **Web dashboard** — Manage everything from a browser: submit jobs, browse workspace files, view rendered markdown reports, download results.

## Install (Production)

Requires Docker and Claude Code authenticated on the host.

```bash
# Authenticate Claude Code (one-time)
claude

# Run the installer
curl -fsSL https://raw.githubusercontent.com/rob-linton/clawmachine/main/scripts/install.sh | bash
```

The installer:
1. Prompts for server IP and admin credentials
2. Pulls all container images (API, worker, scheduler, sandbox)
3. Starts services with Docker Compose
4. Sets up TLS via Caddy

Dashboard opens at `http://<your-ip>`.

## How It Works

### Docker Isolation

Every job runs inside a disposable Docker container:

```
Worker Container                  Sandbox Container (per job)
┌──────────────────┐             ┌─────────────────────┐
│  claw-worker     │──spawns──>  │  claw-sandbox        │
│  (orchestrator)  │             │  - Claude Code CLI   │
│  - claims jobs   │             │  - Node.js 20        │
│  - streams logs  │             │  - git, gh CLI       │
│  - manages git   │             │  - /workspace mount  │
└──────────────────┘             └─────────────────────┘
        │                                 │
        └── Docker socket ───────────────-┘
```

Each sandbox container gets:
- **Memory limit** (default 4GB, configurable per workspace)
- **CPU limit** (default 2.0 cores)
- **PID limit** (256, prevents fork bombs)
- **Network mode** (bridge for API access, or none for full isolation)
- **Workspace mount** — only the job's workspace directory is visible
- **Claude auth** — host credentials mounted read-write

The worker runs as root (for Docker socket access) but sandbox containers run as the authenticated user (non-root). Claude Code refuses `--dangerously-skip-permissions` as root, so this is enforced.

### Git-Backed Workspaces

Workspaces use git as the version control layer — like VMware snapshots but for code:

```
~/.claw-data/
├── repos/           # Bare git repos (source of truth)
│   └── {uuid}.git
├── checkouts/       # Working copies (for file browser)
│   └── {uuid}/
└── jobs/            # Temporary job working dirs
    └── {job-uuid}/
```

**Three persistence modes:**

| Mode | Behavior | Use Case |
|------|----------|----------|
| **Persistent** | Changes accumulate. Full git history. | Long-running projects |
| **Ephemeral** | Fresh clone each job. Changes discarded. | CI tasks, one-off jobs |
| **Snapshot** | Clone from base tag. Results on branches. | A/B testing, approval workflows |

**Fork any workspace** to create a new one starting from its current state. Forks track lineage — you can see which workspace was forked from which, navigate the tree, and create new workspaces from snapshot branches.

Every file browser edit, job execution, fork, and sync is recorded in a **workspace event timeline** — a human-readable history of everything that happened.

### Pipelines

Chain multiple jobs together. Each step's output becomes context for the next:

```json
{
  "name": "Weekly Report",
  "steps": [
    {"prompt": "Analyze the Splunk error logs and summarize findings"},
    {"prompt": "Using {{previous_result}}, create a formatted report with charts"},
    {"prompt": "Using {{previous_result}}, email the report to the team"}
  ],
  "workspace_id": "..."
}
```

Steps execute sequentially in the same workspace. Artifacts from step 1 (files, data) are available to step 2.

### Schedules

Run jobs on cron schedules:

```json
{
  "name": "Nightly Error Report",
  "schedule": "0 0 2 * * *",
  "prompt": "Generate the daily error report from Splunk",
  "workspace_id": "...",
  "enabled": true
}
```

The scheduler checks every 30 seconds and fires jobs when due. Deduplication prevents overlapping runs — if the previous job is still running, the next fire is skipped.

## Dashboard

The Flutter web dashboard provides:

- **Job management** — Submit, monitor, cancel, view results with real-time log streaming
- **Workspace browser** — File tree, inline editor, markdown preview, image preview, ZIP upload/download
- **Workspace forking** — Create new workspaces from existing ones, view lineage
- **Event timeline** — Human-readable history of workspace events (jobs, forks, file edits)
- **Git history** — View commits, revert changes, promote snapshots
- **Settings** — Configure Docker execution, resource limits, credential mounts

## Development Setup

```bash
git clone https://github.com/rob-linton/clawmachine.git
cd clawmachine

# Prerequisites: Rust 1.83+, Flutter 3.x+, Redis, Claude Code CLI
# Start Redis: docker run -d -p 6379:6379 redis:7-alpine
# Authenticate: claude

# Start everything
./scripts/startup.sh
```

**Two-terminal setup for development:**

```bash
# Terminal 1: Backend (API + Worker + Scheduler)
./scripts/backend.sh

# Terminal 2: Frontend (Flutter hot reload on :3000)
./scripts/frontend.sh
```

## API

Full REST API with session cookies or bearer token auth:

```bash
# Submit a job
curl -X POST http://localhost:8080/api/v1/jobs \
  -H "Content-Type: application/json" \
  -d '{"prompt": "Analyze this codebase", "workspace_id": "..."}'

# Create a workspace
curl -X POST http://localhost:8080/api/v1/workspaces \
  -H "Content-Type: application/json" \
  -d '{"name": "My Project", "persistence": "persistent"}'

# Fork a workspace
curl -X POST http://localhost:8080/api/v1/workspaces/{id}/fork \
  -d '{"name": "Experiment Branch"}'

# Download workspace as ZIP
curl http://localhost:8080/api/v1/workspaces/{id}/download -o workspace.zip

# Create a schedule
curl -X POST http://localhost:8080/api/v1/crons \
  -H "Content-Type: application/json" \
  -d '{"name": "Daily", "schedule": "0 0 9 * * *", "prompt": "Run checks"}'
```

See [CLAUDE.md](CLAUDE.md) for full API reference.

## Architecture

```
claw-models  ──>  claw-redis  ──>  claw-api
                      |            claw-worker
                      |            claw-scheduler
                      |            claw-cli
```

| Crate | Purpose |
|-------|---------|
| **claw-models** | Shared types (Job, Workspace, Skill, Pipeline, WorkspaceEvent) |
| **claw-redis** | Redis client with atomic Lua scripts for job claiming and workspace locking |
| **claw-api** | Axum REST API, Flutter static serving, WebSocket events |
| **claw-worker** | Claims jobs, spawns Docker sandboxes, streams logs, manages git |
| **claw-scheduler** | Cron engine (30s tick) + file watcher for .job file submission |
| **claw-cli** | Command-line interface for job submission and management |

## Configuration

Most configuration is managed from the **Settings** screen in the dashboard. Environment variables are used for bootstrap:

| Variable | Default | Purpose |
|----------|---------|---------|
| `CLAW_REDIS_URL` | `redis://127.0.0.1:6379` | Redis connection |
| `CLAW_API_PORT` | `8080` | API server port |
| `CLAW_EXECUTION_BACKEND` | `docker` | `local` or `docker` |
| `CLAW_DATA_DIR` | `~/.claw-data` | Workspace data directory |
| `CLAW_WORKER_CONCURRENCY` | `1` | Parallel jobs per worker |

## License

MIT
