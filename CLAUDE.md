# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

ClaudeCodeClaw is a job queue orchestrator for Claude Code. It wraps `claude -p` in a structured system: jobs are submitted via CLI/API/webhooks/cron/file-drops, parallel workers claim and execute them, and results flow back through Redis to a Flutter dashboard.

**Stack**: Rust (Axum) backend, Flutter (Riverpod) frontend, Redis (queue + state + streams), Docker Compose deployment.

**Auth**: Claude Code uses OAuth — workers inherit the host user's logged-in session (`~/.claude/`). No API key needed.

## Current State

The project is in **Phase 0** (MVP prototype). A single Rust binary polls Redis and runs `claude -p`. The full architecture (6 Rust crates, Flutter app, Docker) is documented in `Documents/architecture/` but not yet implemented.

## Build & Run

```bash
cargo build                          # Build prototype
cargo run                            # Start worker (needs Redis + Claude Code OAuth session)
cargo check                          # Type-check without building
cargo clippy -- -D warnings          # Lint
cargo test                           # Run tests (none yet in Phase 0)
```

## Scripts

```bash
./scripts/submit.sh "your prompt"              # Submit a job to Redis
./scripts/result.sh <job_id>                   # Check job status and result
./scripts/smoke-test-p0.sh                     # End-to-end smoke test (needs running worker)
```

## Prerequisites

- Redis running locally (`redis-server` or `docker run -d -p 6379:6379 redis:7-alpine`)
- Claude Code CLI installed and authenticated via OAuth (`claude --version`)
- Rust toolchain (`rustc 1.83+`)

## Architecture

Full architecture docs live in `Documents/architecture/` (11 documents). Key references:

| Topic | Document |
|-------|----------|
| System overview & decisions | `00-overview.md` |
| Redis schema, Lua scripts, job state machine | `02-data-model.md` |
| REST API + WebSocket spec | `03-api-specification.md` |
| Worker subprocess management | `04-worker-engine.md` |
| Skill injection mechanics | `05-skills-system.md` |
| Implementation phases & self-testing | `10-implementation-roadmap.md` |

### Target Architecture (not yet built)

```
CLI → Axum API → Redis ← Workers (claude -p subprocess)
                   ↑
              Scheduler (cron + file watcher)
                   ↓
            Flutter UI (WebSocket)
```

**Crate structure** (Phase 1+): `claw-models` → `claw-redis` → `claw-api`, `claw-worker`, `claw-scheduler`, `claw-cli`

### Key Design Decisions

- **CLI goes through API** (not direct Redis) — single codepath for validation/events/auth
- **Redis Streams** for job state events (not Pub/Sub) — prevents stale UI from missed events. Pub/Sub only for log lines.
- **Atomic Lua scripts** for job claiming and reaper re-queuing — prevents races between parallel workers
- **Skill snapshotting** — `skill_snapshot` + `assembled_prompt` stored per-job for reproducibility
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

## Self-Testing Rule

Every phase must be validated end-to-end before proceeding. After writing code, exercise it as a real user: hit the API with curl, submit jobs via CLI, open the UI in a browser. See `10-implementation-roadmap.md` section 10 for the full testing protocol.

## Environment Variables

| Variable | Default | Purpose |
|----------|---------|---------|
| `CLAW_REDIS_URL` | `redis://127.0.0.1:6379` | Redis connection |
| `CLAW_API_URL` | `http://127.0.0.1:8080` | API server URL (for CLI) |
