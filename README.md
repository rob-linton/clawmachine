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

- **Interactive chat with infinite memory** — Each user gets a persistent AI colleague that maintains its own notebook, learns your preferences, tracks your projects, and develops genuine understanding over time. Unlike typical AI memory (flat fact extraction), the system uses a cognitive pipeline that reflects on conversations, anticipates your needs, and consolidates knowledge while idle — like a colleague who thinks about your work between meetings.
- **Docker isolation** — Every job runs in its own sandbox container with memory, CPU, and network limits. No job can affect the host or other jobs.
- **Git-backed workspaces** — Workspaces are versioned with git. Fork workspaces, revert to any commit, create snapshots, and track full history.
- **Pipelines** — Chain jobs together. Each step passes context to the next. Results from step 1 become input for step 2.
- **Schedules** — Run jobs on cron schedules. Generate daily reports, run nightly analysis, poll APIs on intervals.
- **Real-time streaming** — Watch Claude work in real-time via the web dashboard. Logs stream as they're produced.
- **Web dashboard** — Manage everything from a browser: submit jobs, browse workspace files, chat with Claude, view reports, download results.

## Install (Production)

Requires Docker and Claude Code authenticated on the host (or an Anthopic API Key).

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

### Interactive Chat with Infinite Memory

Each user gets a persistent AI assistant that **never forgets**. Chat naturally, and the system builds a lasting understanding of who you are, what you're working on, and how you like to work.

```
You: Can you help me fix the auth bug Alice found?

Claude: I remember — Alice flagged the JWT refresh issue last Tuesday (see
.notebook/topics/authentication.md). You chose JWT over sessions three weeks
ago because of the microservice architecture. Let me look at the token
rotation code...
```

**Why we can do this better than anyone else:**

Most AI memory systems are limited to prompt injection — they prepend facts to the system prompt and hope for the best. Claw Machine controls the **entire execution environment**. We own the workspace filesystem, the CLAUDE.md file, the Docker container, and the background processing pipeline. This gives us three levers nobody else has:

1. **We rewrite CLAUDE.md before every message.** Claude Code re-reads it on each invocation. We dynamically assemble it with temporal context, the user's profile, importance-scored memories, and anticipation notes. Claude doesn't experience this as "retrieval" — it experiences it as innate knowledge.

2. **The workspace is the brain.** Claude reads and writes `.notebook/` files naturally — the same way it already knows how to use files. We persist those files to Redis between sessions and restore them on demand. Claude's memory isn't a database it queries; it's a notebook on its desk.

3. **We run background processes between messages.** After each exchange, a cognitive pipeline extracts knowledge. On idle, a consolidation pass refines and synthesizes. No other chat system thinks while you're away.

**How it works:**

The chat runs Claude Code in persistent Docker session containers with `--continue` for native conversation context. Between messages, the system maintains a three-tier memory architecture:

```
┌─────────────────────────────────────────────────┐
│  Tier 1: CLAUDE.md — rewritten every message    │
│  Temporal context, user profile, top memories   │
├─────────────────────────────────────────────────┤
│  Tier 2: .notebook/ — workspace files           │
│  Structured notes Claude reads and writes       │
├─────────────────────────────────────────────────┤
│  Tier 3: Redis — full message archive           │
│  Summaries, mood history, anticipation          │
└─────────────────────────────────────────────────┘
```

**The notebook** — Claude maintains structured notes like a real colleague: `about-user.md`, `active-projects.md`, `decisions.md`, `people.md`, `timeline.md`, and topic-specific deep notes. These persist across sessions and survive chat deletion.

**Cognitive pipeline** — After each message, a background process analyzes the exchange in four stages: extract facts, connect to existing knowledge, assess conversation mood, and anticipate what you'll need next. Results are injected into the next message's context.

**Container restart recovery** — When the Docker container is recycled, the system detects it and injects a "Previously On..." narrative so Claude picks up seamlessly. The notebook is restored from Redis, and the dynamic CLAUDE.md provides full context.

**Temporal awareness** — Claude knows today's date, how long you've been chatting, when your last session was, and upcoming deadlines from your timeline.

**What makes this different from other AI memory systems:**

| Feature | Typical AI Memory | Claw Machine |
|---------|-------------------|--------------|
| Storage | Flat key-value facts | Structured notebook (decisions, people, projects, timeline) |
| Context | Static system prompt | Dynamic CLAUDE.md rewritten every message with time-aware context |
| Memory source | System extracts facts | Bidirectional — Claude writes notes + system extracts + idle consolidation refines |
| Between messages | Nothing | Consolidation pass: merge notes, synthesize understanding, archive stale entries |
| After downtime | Context lost | "Previously On..." narrative recap with zero disruption |
| Session deletion | Memory lost | Notebook survives — it's per-user, not per-session |

See [Documents/PersonalAI.md](Documents/PersonalAI.md) for the full architecture.

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

[MIT](LICENSE) — see [NOTICE.md](NOTICE.md) for trademark attributions.
