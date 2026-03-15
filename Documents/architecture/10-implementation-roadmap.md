# Implementation Roadmap

## 1. Phased Delivery

The project is delivered in 8 phases (0-7). Each phase produces a working, testable increment. No phase depends on a future phase — the system is usable after each phase completes.

## 1.5 Phase 0: Minimal Viable Prototype

**Goal**: Get a working job queue in the minimum possible time to validate the architecture and learn what you actually need.

**Delivers**: A single Rust binary that polls Redis, runs `claude -p`, stores results. Usable within hours, not weeks.

### Tasks

| # | Task | Detail |
|---|------|--------|
| 0.1 | Single-file prototype | ~200 lines of Rust: BLPOP from `claw:queue:pending`, spawn `claude -p`, HSET result |
| 0.2 | Simple submit script | Bash or Rust: push a job JSON to Redis list |
| 0.3 | Simple result viewer | Bash or Rust: read job result from Redis |

### Verification

```bash
# Start Redis
docker run -d -p 6379:6379 redis:7-alpine

# Start prototype worker
ANTHROPIC_API_KEY=sk-... cargo run

# Submit a job (via redis-cli)
redis-cli RPUSH claw:queue:pending '{"id":"test-1","prompt":"What is 2+2?"}'

# Check result
redis-cli GET claw:job:test-1:result
```

### Why Phase 0?

Phase 1 requires 4 crates and 18 files before you can run a single job. Phase 0 gets you a working system in hours so you can:
- Validate that `claude -p` subprocess management works as expected
- Discover edge cases in the stream-json output format
- Build intuition for what the full system actually needs
- Stay motivated with fast tangible progress

After ~1 week of using Phase 0, refactor into the full crate structure with confidence.

### Files Created

```
src/main.rs              # Single-file prototype (all-in-one)
scripts/submit.sh        # Helper to submit jobs
scripts/result.sh        # Helper to read results
```

---

## 2. Phase 1: Foundation

**Goal**: Submit a job via CLI through the API, have it executed by a worker, retrieve the result.

**Delivers**: Minimal viable system — `claw submit` → API → Redis → worker → `claw result`.

### Tasks

| # | Task | Crate | Detail |
|---|------|-------|--------|
| 1.1 | Create Cargo workspace | root | `Cargo.toml` with workspace members, `[workspace.dependencies]` |
| 1.2 | Implement `claw-models` | claw-models | `Job`, `JobStatus`, `OutputDest`, `JobSource`, `Skill`, `SkillType` structs with serde derives |
| 1.3 | Implement Redis pool | claw-redis | `deadpool-redis` pool setup, config from env |
| 1.4 | Implement job submit | claw-redis | `submit_job()`: HSET + RPUSH to `pending:5` + XADD to stream |
| 1.5 | Implement job claim | claw-redis | Lua script: LPOP from priority queues + SADD running |
| 1.6 | Implement job complete/fail | claw-redis | `complete_job()`, `fail_job()`: state transitions in Redis |
| 1.7 | Implement job queries | claw-redis | `get_job()`, `list_jobs()`, `get_result()` |
| 1.8 | Implement minimal API | claw-api | Axum server with `POST /api/v1/jobs`, `GET /api/v1/jobs`, `GET /api/v1/jobs/{id}`, `GET /api/v1/jobs/{id}/result`, `GET /api/v1/status` |
| 1.9 | Implement CLI `submit` | claw-cli | HTTP client calling `POST /api/v1/jobs` |
| 1.10 | Implement CLI `status` | claw-cli | Queue overview via `GET /api/v1/status`, job detail via `GET /api/v1/jobs/{id}` |
| 1.11 | Implement CLI `result` | claw-cli | Fetch result via `GET /api/v1/jobs/{id}/result` |
| 1.12 | Implement CLI `list` | claw-cli | List jobs via `GET /api/v1/jobs` |
| 1.13 | Implement executor | claw-worker | `execute_job()`: spawn `claude -p`, capture output, store skill snapshot |
| 1.14 | Implement worker loop | claw-worker | Claim → execute → complete/fail loop |
| 1.15 | Implement output handler | claw-worker | Route results to Redis / file |
| 1.16 | Write `scripts/smoke-test.sh` | scripts | End-to-end: submit job → poll status → check result → verify via CLI |
| 1.17 | **Self-test** | — | Run smoke test. Submit 3 real jobs with different prompts. Verify all complete with correct results. Fix anything broken before moving on. |

### Verification

```bash
# Start Redis
docker run -d -p 6379:6379 redis:7-alpine

# Start API server
cargo run -p claw-api &

# Start worker
ANTHROPIC_API_KEY=sk-... cargo run -p claw-worker &

# Submit a job (CLI → API → Redis)
cargo run -p claw-cli -- submit "What is 2 + 2?"

# Check status
cargo run -p claw-cli -- status

# Get result (after job completes)
cargo run -p claw-cli -- result <job_id>

# Also works via curl
curl -X POST http://localhost:8080/api/v1/jobs -H 'Content-Type: application/json' -d '{"prompt":"hello"}'

# List all jobs
cargo run -p claw-cli -- list
```

### Files Created

```
Cargo.toml
Cargo.lock
crates/claw-models/Cargo.toml
crates/claw-models/src/lib.rs
crates/claw-models/src/job.rs
crates/claw-models/src/skill.rs
crates/claw-models/src/output.rs
crates/claw-redis/Cargo.toml
crates/claw-redis/src/lib.rs
crates/claw-redis/src/pool.rs
crates/claw-redis/src/jobs.rs
crates/claw-redis/src/lua/claim_job.lua
crates/claw-redis/src/lua/complete_job.lua
crates/claw-redis/src/lua/reaper_requeue.lua
crates/claw-api/Cargo.toml
crates/claw-api/src/main.rs
crates/claw-api/src/app_state.rs
crates/claw-api/src/routes/mod.rs
crates/claw-api/src/routes/jobs.rs
crates/claw-api/src/routes/status.rs
crates/claw-worker/Cargo.toml
crates/claw-worker/src/main.rs
crates/claw-worker/src/executor.rs
crates/claw-worker/src/output_handler.rs
crates/claw-cli/Cargo.toml
crates/claw-cli/src/main.rs
crates/claw-cli/src/api_client.rs
crates/claw-cli/src/submit.rs
crates/claw-cli/src/status.rs
crates/claw-cli/src/config.rs
```

---

## 3. Phase 2: WebSocket + Live Updates + Full API

**Goal**: WebSocket for real-time events, remaining API endpoints, live log streaming.

**Delivers**: Complete REST API, live WebSocket updates, ready for UI.

### Tasks

| # | Task | Crate | Detail |
|---|------|-------|--------|
| 2.1 | Add remaining REST endpoints | claw-api | DELETE jobs, cancel, logs endpoints |
| 2.2 | Implement Redis Streams consumer | claw-redis | `XREAD` with consumer group for job state events (reliable, resumable) |
| 2.3 | Implement Redis Pub/Sub for logs | claw-redis | Subscribe to `claw:events:logs:*` for real-time log lines |
| 2.4 | Implement WebSocket handler | claw-api | Upgrade, subscription management, Streams→WS bridge for job events, PubSub→WS for logs |
| 2.5 | Implement worker log streaming | claw-worker | stdout → RPUSH log list + PUBLISH to pub/sub channel |
| 2.6 | Implement heartbeat + reaper | claw-worker | TTL heartbeat keys, atomic Lua reaper with leader lease |
| 2.7 | Add CLI `logs` command | claw-cli | View logs via `GET /api/v1/jobs/{id}/logs`, `--follow` via WebSocket |
| 2.8 | Add CLI `cancel` command | claw-cli | Cancel via `POST /api/v1/jobs/{id}/cancel` |
| 2.9 | Implement failure webhook | claw-worker | POST to `CLAW_FAILURE_WEBHOOK_URL` on job failure |
| 2.10 | **Self-test** | — | Open `websocat ws://localhost:8080/api/v1/ws`, subscribe to jobs, submit a job via CLI, verify live events appear. Stream logs for a running job. Test cancel. |

### Verification

```bash
# Start API server
cargo run -p claw-api &

# Submit via API
curl -X POST http://localhost:8080/api/v1/jobs \
  -H 'Content-Type: application/json' \
  -d '{"prompt": "Hello world"}'

# List via API
curl http://localhost:8080/api/v1/jobs

# System status
curl http://localhost:8080/api/v1/status

# WebSocket test (requires websocat)
websocat ws://localhost:8080/api/v1/ws
> {"type":"subscribe","channel":"jobs"}
# (observe job events in real-time)

# Stream logs via CLI
cargo run -p claw-cli -- logs <job_id> --follow
```

### Files Created

```
crates/claw-api/src/ws.rs
crates/claw-redis/src/streams.rs    # Redis Streams consumer for job events
crates/claw-redis/src/pubsub.rs     # Redis Pub/Sub for log lines only
crates/claw-cli/src/logs.rs
```

---

## 4. Phase 3: Skills System

**Goal**: Create, store, and inject skills into job prompts.

**Delivers**: Reusable knowledge layer, skill CRUD via API and CLI.

### Tasks

| # | Task | Crate | Detail |
|---|------|-------|--------|
| 3.1 | Implement skill Redis CRUD | claw-redis | `create_skill()`, `get_skill()`, `list_skills()`, `update_skill()`, `delete_skill()` |
| 3.2 | Implement prompt builder | claw-worker | Resolve skills, inject templates, write CLAUDE.md, deploy scripts |
| 3.3 | Implement skill REST endpoints | claw-api | CRUD for skills |
| 3.4 | Implement skill CLI commands | claw-cli | `claw skill create/list/show/edit/delete` |
| 3.5 | Implement filesystem seeding | claw-worker | Load skills from `skills/` directory into Redis on startup |
| 3.6 | Create built-in skills | skills/ | code-review, security-audit, rust-project, run-tests templates |
| 3.7 | Add CLAUDE.md cleanup | claw-worker | Restore original CLAUDE.md after job completes (crash recovery via marker files) |
| 3.8 | Add script cleanup | claw-worker | Remove `.claw/scripts/{job_id}/` after job completes |
| 3.9 | **Self-test** | — | Create a skill via CLI, submit a job referencing it, verify skill appears in logs and in the `skill_snapshot` field. Kill worker mid-job, restart, verify CLAUDE.md was cleaned up. |

### Verification

```bash
# Create a skill
cargo run -p claw-cli -- skill create --id code-review --name "Code Review" \
  --type template --file skills/templates/code-review.md --tags review

# List skills
cargo run -p claw-cli -- skill list

# Submit job with skill
cargo run -p claw-cli -- submit --wait --skill code-review \
  "Review the code in src/main.rs"

# Verify skill was injected (check logs)
cargo run -p claw-cli -- logs <job_id>
# Should show <skill name="code-review"> in the prompt
```

### Files Created

```
crates/claw-redis/src/skills.rs
crates/claw-worker/src/prompt_builder.rs
crates/claw-api/src/routes/skills.rs
crates/claw-cli/src/skills.rs
skills/templates/code-review.md
skills/templates/security-audit.md
skills/claude-configs/rust-project.md
skills/scripts/run-tests.sh
```

---

## 5. Phase 4: Flutter UI

**Goal**: Web dashboard with real-time monitoring and job management.

**Delivers**: Browser-based UI for all core operations.

### Tasks

| # | Task | Detail |
|---|------|--------|
| 4.1 | Create Flutter project | `flutter create flutter_ui`, add dependencies |
| 4.2 | Implement data models | Dart models mirroring Rust structs |
| 4.3 | Implement API client | dio-based REST client |
| 4.4 | Implement WebSocket service | Connection management, auto-reconnect, event parsing |
| 4.5 | Implement Riverpod providers | Jobs, skills, stats, WebSocket bridge |
| 4.6 | Implement AppShell | Navigation sidebar, responsive layout |
| 4.7 | Implement Dashboard screen | Stat cards, queue chart, worker indicators, activity feed |
| 4.8 | Implement Jobs screen | Filterable job list with status badges |
| 4.9 | Implement Job Detail screen | Metadata, streaming log viewer, result display |
| 4.10 | Implement Submit Job screen | Form with skill picker, model selector, etc. |
| 4.11 | Implement Skills screen | Grid of skill cards |
| 4.12 | Implement Skill Editor | Code editor with save/delete |
| 4.13 | Implement Settings screen | Connection config, theme toggle |
| 4.14 | Add static file serving | API server serves Flutter web build via `tower-http` |
| 4.15 | Set up Playwright E2E tests | `tests/e2e/` — install Playwright, write test for submit → complete → view result |
| 4.16 | **Self-test** | — | Open the UI in a browser. Submit a job through the form. Watch logs stream live. See result appear. Create a skill. Check dashboard updates. Run Playwright suite. Fix anything broken before moving on. |

### Verification

```bash
# Development (hot reload)
cd flutter_ui && flutter run -d chrome

# Production build
cd flutter_ui && flutter build web --release

# Copy to API server static dir
cp -r flutter_ui/build/web/* static/

# Access at http://localhost:8080/
# Verify:
# - Dashboard shows live stats
# - Submit a job through the UI
# - Watch logs stream in real-time
# - View completed result
# - Create/edit a skill
```

### Files Created

Full Flutter project structure as described in `07-flutter-ui.md`.

---

## 6. Phase 5: Scheduler

**Goal**: Cron-based scheduled jobs and file watcher ingestion.

**Delivers**: Automated job triggers without manual submission.

### Tasks

| # | Task | Crate | Detail |
|---|------|-------|--------|
| 5.1 | Create `claw-scheduler` crate | claw-scheduler | Tokio entrypoint, config |
| 5.2 | Implement cron Redis CRUD | claw-redis | `create_cron()`, `get_cron()`, etc. |
| 5.3 | Implement cron engine | claw-scheduler | `tokio-cron-scheduler` wrapper, reads from Redis |
| 5.4 | Implement cron sync | claw-scheduler | Periodic re-read of cron defs from Redis |
| 5.5 | Implement file watcher | claw-scheduler | `notify`-based directory watcher, .job file parsing (ignores .tmp files, atomic rename convention) |
| 5.5b | Add cron deduplication | claw-scheduler | Check `last_job_id` — don't submit if previous job is still pending/running |
| 5.10 | **Self-test** | — | Create a cron that fires every minute. Watch it create jobs. Drop a `.job` file in the watched directory. Verify it gets picked up, processed, renamed. Trigger a cron manually via UI. Run Playwright cron test. |
| 5.6 | Implement cron REST endpoints | claw-api | CRUD + trigger |
| 5.7 | Implement cron CLI commands | claw-cli | `claw cron create/list/enable/disable/trigger` |
| 5.8 | Add Crons screen to Flutter | flutter_ui | Cron list with toggle + trigger |
| 5.9 | Add Cron Editor screen | flutter_ui | Create/edit cron schedules |

### Verification

```bash
# Start scheduler
cargo run -p claw-scheduler &

# Create a cron (runs every minute for testing)
cargo run -p claw-cli -- cron create \
  --name "Test Cron" \
  --schedule "* * * * *" \
  --prompt "Say the current time"

# Wait 1-2 minutes, verify jobs were created
cargo run -p claw-cli -- list --source cron

# Test file watcher
echo '{"prompt": "Hello from a file"}' > jobs/test.job
# Wait a few seconds
cargo run -p claw-cli -- list --source filewatcher
ls jobs/  # Should show test.job.submitted
```

### Files Created

```
crates/claw-scheduler/Cargo.toml
crates/claw-scheduler/src/main.rs
crates/claw-scheduler/src/cron.rs
crates/claw-scheduler/src/watcher.rs
crates/claw-redis/src/crons.rs
crates/claw-api/src/routes/crons.rs
crates/claw-cli/src/crons.rs
flutter_ui/lib/screens/crons_screen.dart
flutter_ui/lib/screens/cron_editor_screen.dart
flutter_ui/lib/providers/crons_provider.dart
flutter_ui/lib/models/cron_schedule.dart
```

---

## 7. Phase 6: Docker + Polish

**Goal**: Production-ready containerized deployment, remaining features.

**Delivers**: `docker compose up` runs the complete system.

### Tasks

| # | Task | Detail |
|---|------|--------|
| 6.1 | Write Dockerfile.backend | Multi-stage: builder → api, worker, scheduler, cli images |
| 6.2 | Write docker-compose.yml | 4 services + Redis + volumes |
| 6.3 | Write .env.example | Template for required environment variables |
| 6.4 | Add graceful shutdown | SIGTERM handlers in all binaries |
| 6.5 | Add priority queue | 10-level priority in claim Lua script |
| 6.6 | Add job retry logic | Re-queue on failure with retry_count |
| 6.7 | Add webhook output handler | POST results to webhook URLs |
| 6.8 | Add inbound webhook endpoints | Generic + GitHub webhook handlers |
| 6.9 | Add job cancellation | Cancel flag in Redis, watched by worker |
| 6.10 | Add job timeout | Per-job configurable timeout with process kill |
| 6.11 | Add data cleanup task | Background task to delete old completed/failed jobs |
| 6.12 | Integration test | Compose up → submit → verify → compose down |
| 6.13 | **Self-test** | — | `docker compose up` from scratch. Submit jobs via CLI, API, webhook, file drop, and cron. Verify all 5 ingestion methods work. Watch dashboard. Scale workers to 3, submit 10 jobs, verify parallel execution. Run full Playwright suite against the Dockerized stack. |

### Verification

```bash
# Build everything
docker compose -f docker/docker-compose.yml build

# Start
ANTHROPIC_API_KEY=sk-... docker compose -f docker/docker-compose.yml up -d

# Verify all healthy
docker compose -f docker/docker-compose.yml ps

# Test via API
curl http://localhost:8080/api/v1/status

# Test via CLI (needs Redis access)
claw submit --wait "Hello from Docker"

# Test UI
open http://localhost:8080

# Test webhook
curl -X POST http://localhost:8080/api/v1/webhook/submit \
  -H 'Content-Type: application/json' \
  -d '{"prompt": "Hello from webhook"}'

# Graceful shutdown
docker compose -f docker/docker-compose.yml stop
# Verify workers finish in-flight jobs
```

### Files Created

```
docker/Dockerfile.backend
docker/docker-compose.yml
.env.example
.dockerignore
```

---

## 8. Phase 7: Hardening

**Goal**: Production resilience, observability, and security.

**Delivers**: Battle-hardened system ready for daily use.

### Tasks

| # | Task | Detail |
|---|------|--------|
| 7.1 | Add API authentication | Bearer token auth via Redis-stored keys |
| 7.2 | Add rate limiting | Tower middleware with Redis-backed counters |
| 7.3 | Add Prometheus metrics | `/metrics` endpoint: job counts, latencies, costs |
| 7.4 | Add structured JSON logging | JSON log output in production mode |
| 7.5 | Add cost tracking widgets | Dashboard: daily/weekly cost charts |
| 7.6 | Add desktop Flutter build | macOS + Linux + Windows builds |
| 7.7 | Add job dependency chains | Job B waits for Job A (blocked_by field) |
| 7.8 | Add `--resume` support | Continue a previous Claude Code session |
| 7.9 | Write integration tests | Full lifecycle tests with real Redis |
| 7.10 | Write unit tests | Core logic: prompt builder, Lua scripts, state machine |
| 7.11 | Add GitHub webhook handler | Parse PR events, auto-submit review jobs |
| 7.12 | Add notification webhooks | Alert on job failure, budget exceeded |
| 7.13 | Performance optimization | Connection pooling tuning, batch Redis ops |

### Verification

Run the full test suite:

```bash
# Unit tests
cargo test --workspace

# Integration tests (requires Redis)
cargo test --workspace -- --ignored

# Load test
for i in $(seq 1 50); do
  claw submit "Task $i: analyze a random file" --priority $((RANDOM % 10)) &
done
wait
# Watch dashboard for all 50 jobs processing
```

---

## 9. Dependency Installation Checklist

Before starting Phase 1, ensure these are installed:

```bash
# Rust
rustup update stable
rustc --version    # 1.83+

# Flutter
flutter upgrade
flutter --version  # 3.24+

# Redis
docker run -d --name redis-test -p 6379:6379 redis:7-alpine
redis-cli ping     # PONG

# Claude Code CLI
npm install -g @anthropic-ai/claude-code
claude --version

# Docker
docker --version   # 24+
docker compose version

# Anthropic API key
echo $ANTHROPIC_API_KEY  # Must be set
```

## 10. Continuous Self-Testing — "Use It Like a Human"

Every phase must be validated end-to-end before moving on. Do not write code in isolation — after every meaningful change, exercise the system as an end user would. This is a **mandatory part of every phase**, not a follow-up step.

### 10.1 Testing Philosophy

- **After writing any backend code**: Hit the API with `curl` or the CLI. Verify the response. Check Redis state directly if needed.
- **After writing any UI code**: Open the app in a browser and click through the workflow. Does it actually work? Does the data show up? Does the WebSocket update?
- **After writing any worker code**: Submit a real job and watch it complete. Read the logs. Check the result. Don't just run unit tests — run the actual thing.

### 10.2 API Self-Testing (Phases 0-3, 5-7)

After every backend change, run through the relevant workflow:

```bash
# Submit a job and verify the full lifecycle
JOB_ID=$(curl -s -X POST http://localhost:8080/api/v1/jobs \
  -H 'Content-Type: application/json' \
  -d '{"prompt":"List the files in the current directory"}' | jq -r '.id')

# Poll until complete
while true; do
  STATUS=$(curl -s http://localhost:8080/api/v1/jobs/$JOB_ID | jq -r '.status')
  echo "Status: $STATUS"
  [[ "$STATUS" == "completed" || "$STATUS" == "failed" ]] && break
  sleep 5
done

# Check result
curl -s http://localhost:8080/api/v1/jobs/$JOB_ID/result | jq .

# Verify via CLI too
claw status $JOB_ID
claw result $JOB_ID
```

Write these as shell scripts in `scripts/smoke-test.sh` and run them after every change.

### 10.3 Playwright E2E Testing (Phases 4+)

Once the Flutter UI exists, use Playwright to automate browser-based end-to-end testing. These tests simulate a real user clicking through the app.

**Setup**: `tests/e2e/` directory with Playwright tests targeting the Flutter web build.

```
tests/
└── e2e/
    ├── playwright.config.ts
    ├── package.json
    ├── fixtures/
    │   └── test-setup.ts         # Start services, seed data
    └── specs/
        ├── dashboard.spec.ts     # Dashboard loads, stats display, quick submit works
        ├── submit-job.spec.ts    # Fill form, submit, navigate to detail, see result
        ├── job-detail.spec.ts    # Logs stream in real-time, result renders as markdown
        ├── skills.spec.ts        # Create skill, edit, delete, verify in picker
        ├── crons.spec.ts         # Create cron, toggle, trigger manually
        └── settings.spec.ts      # Change theme, verify persistence
```

**Key E2E test scenarios**:

| Test | What it validates |
|------|-------------------|
| Submit job via UI | Form → submit → redirect to detail → logs stream → result appears |
| Dashboard live updates | Submit job via CLI → dashboard stat cards increment in real-time |
| Job cancellation | Submit long job → click cancel → status changes to cancelled |
| Skill injection | Create skill → submit job with skill → verify skill content in logs |
| Cron trigger | Create cron → click "Trigger Now" → new job appears in jobs list |
| WebSocket reconnection | Kill API → restart → UI reconnects → data refreshes correctly |
| Error states | Submit invalid job → see error message → no crash |

**Example Playwright test**:

```typescript
// tests/e2e/specs/submit-job.spec.ts
import { test, expect } from '@playwright/test';

test('submit a job and see the result', async ({ page }) => {
  await page.goto('http://localhost:8080');

  // Navigate to submit
  await page.click('text=Jobs');
  await page.click('text=New Job');

  // Fill in the form
  await page.fill('[data-testid="prompt-input"]', 'What is 2 + 2?');
  await page.selectOption('[data-testid="model-select"]', 'sonnet');
  await page.fill('[data-testid="budget-input"]', '0.50');

  // Submit
  await page.click('[data-testid="submit-button"]');

  // Should redirect to job detail
  await expect(page).toHaveURL(/\/jobs\/.+/);

  // Wait for completion (status badge changes)
  await expect(page.locator('[data-testid="status-badge"]'))
    .toHaveText('completed', { timeout: 120_000 });

  // Result should be visible
  const result = page.locator('[data-testid="job-result"]');
  await expect(result).toBeVisible();
  await expect(result).toContainText('4');
});

test('dashboard updates in real-time', async ({ page }) => {
  await page.goto('http://localhost:8080');

  // Note current completed count
  const completedBefore = await page
    .locator('[data-testid="stat-completed"]')
    .textContent();

  // Submit a job via API (simulating external trigger)
  const response = await page.request.post('http://localhost:8080/api/v1/jobs', {
    data: { prompt: 'Say hello', max_budget_usd: 0.10 }
  });
  expect(response.ok()).toBeTruthy();

  // Wait for the stat to increment (WebSocket push)
  await expect(page.locator('[data-testid="stat-completed"]'))
    .not.toHaveText(completedBefore!, { timeout: 120_000 });
});
```

### 10.4 When to Run What

| After changing... | Run... |
|-------------------|--------|
| Any Rust code | `cargo test --workspace` + `scripts/smoke-test.sh` |
| API endpoints | `curl` the endpoint manually + check response + smoke test |
| Worker/executor | Submit a real job, watch it complete, verify result |
| Flutter UI | Open browser, click through the workflow you changed |
| Any code in Phase 4+ | Playwright E2E suite: `npx playwright test` |
| Docker config | `docker compose up --build` → full smoke test |
| Before merging/tagging | Full Playwright suite + smoke test + `cargo test` |

### 10.5 Continuous Integration

Once Phase 6 (Docker) is complete, create a CI pipeline that:

1. Builds all Rust crates (`cargo build --workspace`)
2. Runs unit tests (`cargo test --workspace`)
3. Spins up Docker Compose (Redis + API + Worker)
4. Runs `scripts/smoke-test.sh` against the running stack
5. Builds Flutter web, serves it, runs Playwright E2E tests
6. Tears everything down

This ensures no phase breaks previous phases.

### 10.6 Golden Rule

**If you can't demo it working end-to-end, the phase isn't done.** Code that compiles but hasn't been exercised through the API, CLI, or UI as a real user would — is untested code regardless of unit test coverage.

---

## 11. Estimated Scope Per Phase

| Phase | New Files | Modified Files | Complexity |
|-------|-----------|---------------|------------|
| 0. MVP Prototype | ~3 | 0 | Low — single-file worker, validate assumptions |
| 1. Foundation | ~25 | 0 | Medium — core data model + worker loop + minimal API + CLI |
| 2. WebSocket + Live | ~5 | ~6 | Medium — Streams consumer + WebSocket + PubSub bridge |
| 3. Skills | ~10 | ~5 | Medium — prompt builder + skill snapshot is the tricky part |
| 4. Flutter UI | ~30 | ~2 | High — most code by volume, but it's UI |
| 5. Scheduler | ~10 | ~5 | Low-Medium — cron + file watcher + dedup |
| 6. Docker | ~5 | ~10 | Medium — integration and polish across all services |
| 7. Hardening | ~15 | ~15 | High — security, testing, optimization |
