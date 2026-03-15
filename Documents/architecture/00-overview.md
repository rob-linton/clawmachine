# ClaudeCodeClaw — System Overview

## 1. Vision

ClaudeCodeClaw is a production-grade job queue orchestrator for Claude Code. It replaces ad-hoc, manual invocations of `claude -p` with a structured system where:

- Jobs are submitted from multiple sources (CLI, REST API, webhooks, cron schedules, file drops)
- Parallel workers claim and execute jobs by spawning `claude -p` as subprocesses
- A shared skills system provides reusable prompt templates, CLAUDE.md configs, and executable scripts
- All state flows through Redis (queue, metadata, streams/pub-sub, results)
- A Flutter-based web and desktop UI provides real-time monitoring and management
- Docker Compose packages everything for reproducible deployment

## 2. Problem Statement

Claude Code is a powerful agentic CLI tool, but using it for recurring or automated workflows requires:

- **Manual invocation** — someone must open a terminal and run the command
- **No queueing** — multiple tasks pile up with no ordering or prioritization
- **No shared context** — each invocation starts from scratch with no accumulated organizational knowledge
- **No visibility** — no dashboard showing what's running, what completed, or what failed
- **No coordination** — running multiple instances risks duplicate work or conflicting file edits

ClaudeCodeClaw solves all of these by wrapping Claude Code in a proper job orchestration layer.

## 3. Goals

| Goal | Description |
|------|-------------|
| **Automation** | Jobs run without human presence — triggered by schedule, webhook, or queue |
| **Parallelism** | 2-3 workers process jobs concurrently with safe job claiming |
| **Skill Reuse** | Accumulated knowledge (prompt templates, configs, scripts) persists across jobs |
| **Observability** | Real-time visibility into queue depth, running jobs, logs, costs |
| **Flexibility** | Multiple input sources and output destinations, task-agnostic design |
| **Simplicity** | `claw submit "do X"` is all a user needs to know to get started |
| **Reliability** | Dead worker detection, automatic retries, persistent state |

## 4. Non-Goals (Explicitly Out of Scope)

- **Multi-tenancy** — single-user/team system, no tenant isolation
- **Distributed deployment** — designed for single-machine Docker Compose, not Kubernetes
- **Custom LLM backends** — only Claude Code CLI, not arbitrary LLM APIs
- **Persistent conversation** — each job is a clean `claude -p` invocation (skills provide continuity, not chat history)
- **GUI job authoring** — the UI monitors and manages; complex job definitions are authored via CLI or API

## 5. Technology Stack

| Layer | Technology | Rationale |
|-------|-----------|-----------|
| Backend | **Rust** (Axum) | Performance, safety, strong async ecosystem. User is experienced. |
| Frontend | **Flutter** (Riverpod) | Single codebase for web + desktop. Reactive state management. |
| Queue/State | **Redis** 7+ | Streams for reliable events, atomic list ops, TTL keys, sorted sets — all primitives needed. |
| CLI | **clap** (Rust) | Standard Rust CLI framework with derive macros. |
| Process Mgmt | **tokio::process** | Async subprocess spawning and stdout streaming. |
| Deployment | **Docker Compose** | Redis + API + Workers + Scheduler in isolated containers. |

## 6. System Context Diagram

```
                          ┌─────────────────────────────────────────────┐
                          │              ClaudeCodeClaw                  │
                          │                                             │
  ┌──────┐   REST/WS     │  ┌─────────┐     ┌───────┐     ┌────────┐ │
  │ User ├───────────────►│  │ Axum    │────►│ Redis │◄────┤Workers │ │
  │ (UI) │◄──────────────┤  │ API     │◄────┤       │────►│(claude)│ │
  └──────┘                │  └─────────┘     └───┬───┘     └────────┘ │
                          │       ▲               │                    │
  ┌──────┐   CLI          │       │          ┌────┴─────┐              │
  │ User ├────────────────┼───────┘          │Scheduler │              │
  │(term)│                │                  │(cron+fs) │              │
  └──────┘                │                  └──────────┘              │
                          │                                             │
  ┌──────────┐  Webhook   │                                             │
  │ GitHub   ├────────────┼──► /api/v1/webhook/github                   │
  │ Slack    │            │                                             │
  │ etc.     │            │                                             │
  └──────────┘            └─────────────────────────────────────────────┘
                                       │
                                       ▼
                              ┌──────────────┐
                              │ Anthropic API│
                              │ (via claude) │
                              └──────────────┘
```

## 7. Key Architectural Decisions

### 7.1 Separate Binaries over Monolith

The system runs as 4 separate Rust binaries (`claw-api`, `claw-worker`, `claw-scheduler`, `claw-cli`) rather than a single binary with subcommands. This enables:

- **Independent scaling** — run 5 worker containers but 1 API server
- **Isolation** — workers need `claude` CLI installed; API server doesn't
- **Independent restarts** — restart a crashed worker without downtime on the API
- **Security** — workers run with `ANTHROPIC_API_KEY`; API server doesn't need it

### 7.2 Redis as Single State Store

Redis serves triple duty: job queue, shared state, and event bus. This avoids introducing Postgres/RabbitMQ/Kafka for a system that doesn't need their specific guarantees. The trade-off is durability — mitigated by enabling AOF persistence. For this use case (automation jobs, not financial transactions), the simplicity wins.

**Event reliability**: Job state transitions use Redis Streams (not Pub/Sub) so that the API server can resume from the last-read position after a reconnection, preventing stale UI state. Pub/Sub is used only for log lines, where occasional loss is acceptable.

### 7.5 CLI Communicates via REST API

The `claw` CLI sends all commands through the API server's REST endpoints (`POST /api/v1/jobs`, etc.) rather than connecting directly to Redis. This ensures:

- **Single source of truth** for validation, defaults, and event publishing (no duplicated logic)
- **Authentication works end-to-end** when added in Phase 7
- **No Redis network access required** for CLI users — only HTTP to the API server
- **Consistent behavior** — CLI and UI always go through the same codepath

The trade-off is that the CLI requires the API server to be running. For `--follow` log streaming, the CLI connects to the WebSocket endpoint.

### 7.3 Subprocess over Direct API

Workers invoke `claude -p` as a subprocess rather than calling the Anthropic API directly. This preserves Claude Code's full agentic capabilities (file editing, tool use, MCP servers, CLAUDE.md reading) which would need to be reimplemented if calling the API directly.

### 7.4 One-Shot Jobs with Skill Injection

Each job runs as a clean `claude -p` invocation (no persistent conversation state). Continuity between jobs comes from the skills system: shared templates, CLAUDE.md configs, and scripts that shape each invocation's behavior. This avoids context window accumulation issues from long-running sessions.

## 8. Repository Structure

```
claudecodeclaw/
├── Cargo.toml                      # Workspace manifest
├── Cargo.lock
├── config.toml                     # Default configuration
│
├── crates/
│   ├── claw-models/                # Shared types (Job, Skill, Events)
│   ├── claw-redis/                 # Redis abstraction layer
│   ├── claw-api/                   # Axum HTTP/WS server
│   ├── claw-worker/                # Job execution engine
│   ├── claw-scheduler/             # Cron + file watcher
│   └── claw-cli/                   # CLI client
│
├── flutter_ui/                     # Flutter web + desktop app
│   ├── pubspec.yaml
│   ├── lib/
│   └── web/
│
├── skills/                         # Built-in skill library
│   ├── templates/
│   ├── claude-configs/
│   └── scripts/
│
├── docker/
│   ├── Dockerfile.backend
│   ├── Dockerfile.flutter
│   └── docker-compose.yml
│
├── jobs/                           # File watcher input directory
├── output/                         # Default file output directory
│
└── Documents/
    └── architecture/               # This documentation
```
