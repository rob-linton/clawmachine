# System Architecture — Components and Interactions

## 1. Component Diagram

```
┌──────────────────────────────────────────────────────────────────────┐
│                        Docker Compose Network                        │
│                                                                      │
│  ┌──────────────────────────────────────────────────────────────┐   │
│  │                     claw-api  (:8080)                        │   │
│  │                                                              │   │
│  │  ┌────────────┐  ┌────────────┐  ┌───────────┐  ┌────────┐ │   │
│  │  │  REST API   │  │ WebSocket  │  │  Static   │  │ Health │ │   │
│  │  │  Handlers   │  │  Handler   │  │  Files    │  │ Check  │ │   │
│  │  └─────┬──────┘  └─────┬──────┘  │ (Flutter) │  └────────┘ │   │
│  │        │                │         └───────────┘              │   │
│  │        └────────┬───────┘                                    │   │
│  │                 │                                            │   │
│  │        ┌────────┴────────┐                                   │   │
│  │        │  Redis Client   │                                   │   │
│  │        │  (deadpool)     │                                   │   │
│  │        └────────┬────────┘                                   │   │
│  └─────────────────┼────────────────────────────────────────────┘   │
│                    │                                                 │
│           ┌────────┴────────┐                                       │
│           │                 │                                       │
│  ┌────────┴─────────────────┴──────────────────────────────────┐   │
│  │                      Redis  (:6379)                          │   │
│  │                                                              │   │
│  │  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌────────────┐  │   │
│  │  │  Lists   │  │  Hashes  │  │  Sorted  │  │  Pub/Sub   │  │   │
│  │  │ (queues) │  │  (jobs,  │  │  Sets    │  │ (events,   │  │   │
│  │  │          │  │  skills) │  │ (history)│  │   logs)    │  │   │
│  │  └──────────┘  └──────────┘  └──────────┘  └────────────┘  │   │
│  └──────────────────────┬──────────────────────────────────────┘   │
│                         │                                           │
│           ┌─────────────┼─────────────┐                             │
│           │             │             │                             │
│  ┌────────┴──────┐ ┌───┴────────┐ ┌──┴──────────────┐             │
│  │  claw-worker  │ │claw-worker │ │  claw-scheduler  │             │
│  │  (instance 1) │ │(instance 2)│ │                  │             │
│  │               │ │            │ │ ┌──────────────┐ │             │
│  │ ┌───────────┐ │ │┌──────────┐│ │ │ Cron Engine  │ │             │
│  │ │ Task Pool │ │ ││Task Pool ││ │ └──────────────┘ │             │
│  │ │ (2 async) │ │ ││(2 async) ││ │ ┌──────────────┐ │             │
│  │ └─────┬─────┘ │ │└────┬─────┘│ │ │ File Watcher │ │             │
│  │       │       │ │     │      │ │ └──────────────┘ │             │
│  │  ┌────┴────┐  │ │┌────┴────┐ │ └──────────────────┘             │
│  │  │claude -p│  │ ││claude -p│ │                                   │
│  │  │ (child) │  │ ││ (child) │ │                                   │
│  │  └─────────┘  │ │└─────────┘ │                                   │
│  └───────────────┘ └────────────┘                                   │
│                                                                      │
└──────────────────────────────────────────────────────────────────────┘

External:
  ┌──────────┐  REST/WS     ┌──────────┐  REST/WS    ┌──────────┐
  │   CLI    ├─────────────►│ claw-api ├◄────────────┤ Flutter  │
  │  (claw)  │              └──────────┘             │   UI     │
  └──────────┘                                       └──────────┘
```

## 2. Service Descriptions

### 2.1 claw-api (Axum HTTP Server)

**Binary**: `claw-api`
**Port**: 8080
**Responsibilities**:
- Serves the REST API for job/skill/cron CRUD operations
- Manages WebSocket connections for real-time updates
- Bridges Redis Pub/Sub events to WebSocket clients
- Serves the Flutter web build as static files (production)
- Provides health check and system status endpoints
- Receives inbound webhooks (GitHub, generic)

**Does NOT**:
- Execute jobs (that's the worker's job)
- Run cron schedules (that's the scheduler's job)
- Need Claude Code installed
- Need an Anthropic API key

**Internal structure**:
```
claw-api
├── main.rs              # Entrypoint: config, Redis pool, Axum router, graceful shutdown
├── app_state.rs         # AppState struct: Redis pool, broadcast channels, config
├── routes/
│   ├── mod.rs           # Router assembly
│   ├── jobs.rs          # POST/GET/DELETE /api/v1/jobs, cancel, result, logs
│   ├── skills.rs        # CRUD /api/v1/skills
│   ├── crons.rs         # CRUD /api/v1/crons, trigger
│   ├── status.rs        # GET /api/v1/status, /api/v1/workers
│   └── webhook.rs       # POST /api/v1/webhook/{submit,github}
├── ws.rs                # WebSocket upgrade, subscription management, event forwarding
└── static_files.rs      # tower-http ServeDir for Flutter build
```

### 2.2 claw-worker (Job Execution Engine)

**Binary**: `claw-worker`
**Responsibilities**:
- Runs N concurrent async tasks (configurable, default 2)
- Each task polls Redis for pending jobs using atomic Lua claim script
- Builds the full prompt by injecting skills (templates, CLAUDE.md, scripts)
- Spawns `claude -p` as a child process with appropriate flags
- Streams stdout to Redis (log list + pub/sub) for real-time visibility
- Parses final result and routes it to the configured output destination
- Maintains a heartbeat in Redis (TTL key refreshed every 10s)
- Handles graceful shutdown on SIGTERM (finishes current job, then exits)

**Requires**:
- Claude Code CLI installed and on PATH
- `ANTHROPIC_API_KEY` environment variable set
- Network access to Redis
- Filesystem access to working directories and output directories

**Internal structure**:
```
claw-worker
├── main.rs              # Entrypoint: config, spawn N worker tasks + reaper task
├── executor.rs          # Subprocess management: spawn, stream, collect result
├── prompt_builder.rs    # Assembles final prompt from job definition + skills
└── output_handler.rs    # Routes results to file / Redis / webhook
```

### 2.3 claw-scheduler (Cron + File Watcher)

**Binary**: `claw-scheduler`
**Responsibilities**:
- Loads cron job definitions from Redis on startup and watches for changes
- Triggers jobs at scheduled times using `tokio-cron-scheduler`
- Watches a configured directory for `.job` files using the `notify` crate
- Parses `.job` files as JSON, submits them as jobs to Redis, renames to `.job.submitted`
- Periodically re-syncs cron definitions from Redis (handles runtime additions/removals)

**Does NOT**:
- Execute jobs (submits to the queue for workers to claim)
- Serve any HTTP endpoint

**Internal structure**:
```
claw-scheduler
├── main.rs              # Entrypoint: config, launch cron engine + file watcher
├── cron.rs              # tokio-cron-scheduler wrapper, Redis cron CRUD sync
└── watcher.rs           # notify-based directory watcher, .job file parser
```

### 2.4 claw-cli (Command-Line Client)

**Binary**: `claw` (or `claw-cli`)
**Responsibilities**:
- Provides a user-friendly CLI for all system operations
- Communicates with the API server via REST endpoints (all operations go through `claw-api`)
- Supports interactive features: `--wait` blocks until job completes, `--follow` streams logs via WebSocket

**Design decision**: The CLI communicates through the API server rather than directly to Redis. This ensures a single codepath for validation, defaults, and event publishing. It also means authentication (Phase 7) works end-to-end. The trade-off is that the CLI requires the API server to be running.

**Internal structure**:
```
claw-cli
├── main.rs              # clap entrypoint, subcommand dispatch
├── api_client.rs        # HTTP client for REST API (reqwest)
├── submit.rs            # Job submission with all options
├── status.rs            # Queue overview and job detail
├── skills.rs            # Skill CRUD commands
├── logs.rs              # Log viewing with --follow (WebSocket)
└── config.rs            # Config file loading (~/.claw/config.toml)
```

## 3. Crate Dependency Graph

```
                    ┌────────────┐
                    │ claw-models│   Pure types, serde derives
                    │  (library) │   Zero I/O, zero async
                    └─────┬──────┘
                          │
                    ┌─────┴──────┐
                    │ claw-redis │   Redis pool, CRUD, Lua scripts
                    │  (library) │   Async, deadpool-redis
                    └─────┬──────┘
                          │
          ┌───────────────┼───────────────┬───────────────┐
          │               │               │               │
    ┌─────┴──────┐  ┌────┴───────┐ ┌─────┴──────┐ ┌─────┴──────┐
    │  claw-api  │  │claw-worker │ │claw-sched. │ │  claw-cli  │
    │  (binary)  │  │  (binary)  │ │  (binary)  │ │  (binary)  │
    └────────────┘  └────────────┘ └────────────┘ └────────────┘
```

- **claw-models** depends on: `serde`, `serde_json`, `chrono`, `uuid`, `strum`
- **claw-redis** depends on: `claw-models`, `redis`, `deadpool-redis`, `tokio`
- All four binaries depend on: `claw-redis` (which re-exports `claw-models`)
- No binary depends on another binary

## 4. Data Flow Diagrams

### 4.1 Job Submission via CLI

```
User                CLI              API Server          Redis              Worker
 │                   │                  │                  │                  │
 │  claw submit "X"  │                  │                  │                  │
 │──────────────────►│                  │                  │                  │
 │                   │  POST /api/v1/   │                  │                  │
 │                   │  jobs            │                  │                  │
 │                   │─────────────────►│  HSET job:abc    │                  │
 │                   │                  │  RPUSH pending   │                  │
 │                   │                  │  XADD stream     │                  │
 │                   │  201 {id: abc}   │─────────────────►│                  │
 │                   │◄─────────────────│                  │                  │
 │                   │                  │                  │  LPOP pending    │
 │                   │                  │                  │  SADD running    │
 │                   │                  │                  │◄─────────────────│
 │                   │                  │                  │                  │
 │                   │                  │                  │  claude -p "X"   │
 │                   │                  │                  │  (subprocess)    │
 │                   │                  │                  │                  │
 │  claw result abc  │  GET /api/v1/    │                  │                  │
 │──────────────────►│  jobs/abc/result │  GET result      │                  │
 │                   │─────────────────►│─────────────────►│                  │
 │  ◄────────────────│◄─────────────────│◄─────────────────│                  │
 │  "result text"    │                  │                  │                  │
```

### 4.2 Live Updates via WebSocket

```
Flutter UI           API Server          Redis             Worker
    │                    │                  │                 │
    │  WS connect        │                  │                 │
    │───────────────────►│                  │                 │
    │                    │  SUBSCRIBE       │                 │
    │  subscribe:jobs    │  claw:events:*   │                 │
    │───────────────────►│─────────────────►│                 │
    │                    │                  │                 │
    │                    │                  │  PUBLISH        │
    │                    │                  │  job_update     │
    │                    │                  │◄────────────────│
    │                    │◄─────────────────│                 │
    │  {type:job_update} │                  │                 │
    │◄───────────────────│                  │                 │
    │                    │                  │                 │
    │  subscribe:logs:X  │  SUBSCRIBE       │                 │
    │───────────────────►│  claw:events:    │                 │
    │                    │  logs:X          │                 │
    │                    │─────────────────►│                 │
    │                    │                  │  PUBLISH        │
    │                    │                  │  log line       │
    │                    │                  │◄────────────────│
    │                    │◄─────────────────│                 │
    │  {type:job_log}    │                  │                 │
    │◄───────────────────│                  │                 │
```

### 4.3 Cron Job Trigger

```
Scheduler             Redis              Worker
    │                   │                  │
    │  (cron fires)     │                  │
    │                   │                  │
    │  HSET job:xyz     │                  │
    │  RPUSH pending    │                  │
    │  PUBLISH event    │                  │
    │─────────────────►│                   │
    │                   │  (normal claim)   │
    │                   │─────────────────►│
    │                   │                  │ claude -p ...
    │                   │                  │
```

### 4.4 File Watcher Flow

```
Filesystem           Scheduler            Redis
    │                   │                   │
    │  new: task.job     │                   │
    │──────────────────►│                   │
    │                   │  parse JSON       │
    │                   │  HSET job         │
    │                   │  RPUSH pending    │
    │                   │─────────────────►│
    │                   │                   │
    │  rename:           │                   │
    │  task.job.submitted│                   │
    │◄──────────────────│                   │
```

## 5. Communication Protocols

### 5.1 Inter-Service Communication

All services communicate exclusively through Redis. There is no direct service-to-service RPC.

| From | To | Mechanism | Purpose |
|------|----|-----------|---------|
| CLI → API | HTTP REST + WebSocket | All operations (submit, status, logs, skills, crons) |
| API → Redis | Direct connection | CRUD operations, WebSocket bridging |
| Worker → Redis | Direct connection | Job claiming, result storage, log streaming |
| Scheduler → Redis | Direct connection | Job submission (cron/filewatcher) |
| Worker → Redis → API → UI | Pub/Sub → WebSocket | Real-time event forwarding |

This architecture means:
- Services can restart independently without affecting others
- No service discovery needed — everything talks to Redis
- Adding a new worker is just starting another container
- The system degrades gracefully: if the API is down, workers still process jobs

### 5.2 External Communication

| External System | Protocol | Endpoint |
|----------------|----------|----------|
| Flutter UI (browser) | HTTP + WebSocket | `http://host:8080/` |
| Flutter UI (desktop) | HTTP + WebSocket | `http://localhost:8080/api/` |
| CLI | HTTP REST + WebSocket | `http://host:8080/api/v1/` |
| GitHub webhooks | HTTP POST | `/api/v1/webhook/github` |
| Generic webhooks | HTTP POST | `/api/v1/webhook/submit` |
| Job result callbacks | HTTP POST | Configurable per-job URL |

## 6. Concurrency Model

### 6.1 API Server

- Single Axum server handling requests on a tokio multi-threaded runtime
- Each REST request gets a task from the tokio executor pool
- WebSocket connections are long-lived tasks, one per connected client
- A dedicated background task reads from Redis Streams (`XREAD` with consumer groups) for job state events, and subscribes to Redis Pub/Sub for log lines. Both are broadcast to WebSocket clients via `tokio::sync::broadcast`
- On reconnection, the Streams consumer resumes from the last acknowledged ID, preventing missed events
- A periodic stats aggregation task runs every 5 seconds, publishes to the broadcast channel

### 6.2 Worker

- Single tokio runtime, multi-threaded
- N worker tasks run concurrently (N = `CLAW_WORKER_CONCURRENCY`, default 2)
- Each worker task: polls Redis → spawns `claude -p` → streams output → stores result
- A heartbeat task runs every 10 seconds, refreshing TTL keys for all active workers
- A reaper task checks for dead workers every 15 seconds, using an atomic Lua script to prevent duplicate re-queuing across multiple worker instances
- Only one reaper is active at a time, enforced via Redis leader lease (`SETNX` with TTL)
- `claude -p` subprocesses run outside tokio — stdout is read via `tokio::io::AsyncBufReadExt`

### 6.3 Scheduler

- Single tokio runtime
- Cron engine runs as a managed task from `tokio-cron-scheduler`
- File watcher runs on a dedicated thread (required by the `notify` crate), communicates to async context via `tokio::sync::mpsc`
- A sync task periodically re-reads cron definitions from Redis (every 60 seconds)

## 7. Error Handling Strategy

### 7.1 Job Failures

| Failure Mode | Detection | Recovery |
|-------------|-----------|----------|
| `claude -p` exits non-zero | Exit code check | Mark job as failed, store stderr in error field |
| `claude -p` hangs | Configurable timeout (default 30 min) | Kill process, mark failed |
| Worker crashes mid-job | Heartbeat TTL expires | Reaper re-queues job (up to 3 retries) |
| Worker OOM killed | Heartbeat TTL expires | Same as crash |
| Redis connection lost | Connection pool error | Worker retries with exponential backoff, then exits |
| Output webhook fails | HTTP error / timeout | Log error, store result in Redis as fallback |

### 7.2 System Failures

| Failure Mode | Impact | Recovery |
|-------------|--------|----------|
| Redis down | All services stalled | Services retry connections; Redis AOF restores state on restart |
| API server down | No UI/API access | Workers + scheduler continue processing; restart API independently |
| All workers down | Jobs queue up | Jobs remain in pending queue; start new workers to drain |
| Scheduler down | No cron/filewatcher | Existing queued jobs still process; cron jobs missed until restart |

## 8. Configuration

### 8.1 Environment Variables

All services accept configuration via environment variables (12-factor app style):

```
# Shared
CLAW_REDIS_URL=redis://redis:6379        # Redis connection URL
RUST_LOG=info,claw_api=debug              # Logging level (tracing-subscriber)

# API-specific
CLAW_API_PORT=8080                        # HTTP listen port
CLAW_STATIC_DIR=/app/static              # Flutter web build directory
CLAW_CORS_ORIGINS=*                       # Allowed CORS origins (dev mode)

# Worker-specific
CLAW_WORKER_CONCURRENCY=2                 # Number of concurrent worker tasks
CLAW_WORKER_TIMEOUT_SECS=1800            # Max job execution time (30 min)
CLAW_SKILLS_DIR=/app/skills              # Skills directory for seeding
CLAW_OUTPUT_DIR=/app/output              # Default file output directory
ANTHROPIC_API_KEY=sk-ant-...             # Required for claude CLI

# Scheduler-specific
CLAW_JOBS_WATCH_DIR=/app/jobs            # Directory to watch for .job files
CLAW_CRON_SYNC_INTERVAL_SECS=60         # How often to re-read cron defs from Redis
```

### 8.2 Configuration File (`config.toml`)

```toml
[redis]
url = "redis://127.0.0.1:6379"

[api]
port = 8080
static_dir = "./flutter_ui/build/web"
cors_origins = ["http://localhost:3000"]   # dev flutter

[worker]
concurrency = 2
timeout_secs = 1800
skills_dir = "./skills"
output_dir = "./output"

[scheduler]
watch_dir = "./jobs"
cron_sync_interval_secs = 60

[defaults]
model = "sonnet"
max_budget_usd = 1.00
output = "redis"
priority = 5
```

Precedence: Environment variables > config.toml > hardcoded defaults.
