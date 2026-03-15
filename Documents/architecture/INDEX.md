# ClaudeCodeClaw Architecture Documentation

## Documents

| # | Document | Description |
|---|----------|-------------|
| 00 | [System Overview](00-overview.md) | Vision, goals, technology stack, key decisions, repository structure |
| 01 | [System Architecture](01-system-architecture.md) | Components, crate dependencies, data flows, concurrency model, configuration |
| 02 | [Data Model](02-data-model.md) | Redis schema, job lifecycle state machine, Lua scripts, skill/cron models, retention |
| 03 | [API Specification](03-api-specification.md) | REST endpoints, WebSocket protocol, request/response schemas, Axum router |
| 04 | [Worker Engine](04-worker-engine.md) | Process lifecycle, subprocess execution, log streaming, output routing, heartbeat/reaper |
| 05 | [Skills System](05-skills-system.md) | Skill taxonomy, injection mechanics, storage, composition, built-in library |
| 06 | [CLI Reference](06-cli-reference.md) | All commands and flags, configuration file format, examples |
| 07 | [Flutter UI](07-flutter-ui.md) | Screen designs, Riverpod state management, WebSocket integration, Dart models |
| 08 | [Deployment](08-deployment.md) | Docker Compose, Dockerfile, local dev setup, operations, logging |
| 09 | [Security and Reliability](09-security-and-reliability.md) | Threat model, sandboxing, cost control, failure recovery, authentication roadmap |
| 10 | [Implementation Roadmap](10-implementation-roadmap.md) | 7 phases with tasks, verification steps, and file lists |

## Quick Reference

**Stack**: Rust (Axum) + Flutter (Riverpod) + Redis + Docker Compose

**Binaries**: `claw-api`, `claw-worker`, `claw-scheduler`, `claw` (CLI)

**Key Flows**:
- Submit: CLI/API → Redis pending queue → Worker claims → `claude -p` → Result to Redis/file/webhook
- Live updates: Worker → Redis PubSub → API WebSocket → Flutter UI
- Skills: Templates prepended to prompt, CLAUDE.md written to working dir, scripts deployed to working dir
