# Security and Reliability

## 1. Threat Model

Claw Machine is a single-user/team system running on trusted infrastructure. The primary threats are:

| Threat | Likelihood | Impact | Mitigation |
|--------|-----------|--------|------------|
| Claude Code modifies unintended files | Medium | High | Working directory isolation, `allowed_tools` restrictions |
| API key leakage | Low | Critical | Key in env var only, never logged or stored in Redis |
| Malicious webhook input | Medium | Medium | Input validation, webhook signature verification |
| Redis data loss | Low | Medium | AOF persistence, regular backups |
| Worker escape (container breakout) | Very Low | High | Non-root user, minimal capabilities, Docker isolation |
| Runaway costs | Medium | Medium | Per-job budget limits, daily cost tracking, alerts |

## 2. Claude Code Sandboxing

### 2.1 Permission Model

Claude Code in headless mode (`--dangerously-skip-permissions`) runs with full tool access. This is necessary for automation but means every job has the permissions of the worker process.

**Defense in depth**:

```
Layer 1: Per-job tool restrictions
  └─ --allowed-tools Read,Grep,Glob (read-only jobs)
  └─ --allowed-tools Read,Edit,Write,Bash (full access)

Layer 2: Working directory isolation
  └─ Each job runs in its own working directory
  └─ Workers create temp dirs for jobs without explicit working_dir
  └─ Prevents cross-job file interference

Layer 3: Container filesystem limits
  └─ Only mounted volumes are accessible
  └─ /app/workspaces, /app/output, /app/skills
  └─ Host filesystem is not exposed

Layer 4: Process user
  └─ Worker runs as non-root 'claw' user
  └─ No sudo, no privilege escalation
  └─ Standard Linux DAC applies
```

### 2.2 Tool Restriction Patterns

Common patterns for restricting what Claude can do per job:

```json
// Read-only analysis (safest)
{
    "allowed_tools": ["Read", "Grep", "Glob"]
}

// Code review (read + limited write for comments)
{
    "allowed_tools": ["Read", "Grep", "Glob", "Edit"]
}

// Full code modification
{
    "allowed_tools": ["Read", "Grep", "Glob", "Edit", "Write", "Bash"]
}

// No restrictions (default — full Claude Code capabilities)
{
    "allowed_tools": null
}
```

### 2.3 Working Directory Best Practices

| Scenario | Recommendation |
|----------|---------------|
| Independent tasks | Each job gets its own temp directory |
| Tasks on the same repo | Use git worktrees (one per job) |
| Read-only analysis | Share a directory, restrict to read-only tools |
| File-producing tasks | Unique output subdirectory per job |

## 3. Cost Control

### 3.1 Per-Job Budget

Each job can specify `max_budget_usd`. This is passed to Claude Code which tracks token costs and stops when the budget is exhausted.

```json
{
    "prompt": "Review this codebase",
    "max_budget_usd": 2.00
}
```

### 3.2 Cost Tracking

The system tracks costs at multiple levels:

- **Per job**: `cost_usd` field populated from Claude Code's output JSON
- **Daily aggregate**: `claw:stats:daily:{date}:cost_usd` (INCRBYFLOAT)
- **Total aggregate**: `claw:stats:total_cost_usd` (INCRBYFLOAT)

### 3.3 Failure and Cost Notifications

The system supports proactive alerting via a global `CLAW_FAILURE_WEBHOOK_URL` config:

**Failure notifications** (sent immediately when a job enters FAILED state):
```json
{
    "type": "job_failed",
    "job_id": "abc-123",
    "prompt": "Review PR #42...",
    "error": "claude exited with code 1",
    "source": "cron",
    "cron_name": "Morning PR Review",
    "retry_count": 0,
    "timestamp": "2026-03-15T02:15:00Z"
}
```

**Cost alerts** (planned, sent when thresholds are exceeded):
- A single job exceeds a cost threshold
- Daily aggregate cost exceeds a budget
- Weekly aggregate cost exceeds a budget

This is critical for unattended operation (cron jobs, overnight automation) where nobody is watching the dashboard.

### 3.4 Execution Timeout

Every job has a timeout (default 30 minutes, configurable per-job). When the timeout is reached:

1. The worker sends SIGTERM to the `claude` child process
2. Waits 10 seconds for graceful exit
3. Sends SIGKILL if still running
4. Marks the job as failed with "timeout" error

This prevents runaway jobs from accumulating costs indefinitely.

## 4. Data Protection

### 4.1 Sensitive Data in Prompts

Job prompts and results may contain sensitive information (code, credentials, internal data). Mitigations:

- **Redis**: Protected by network isolation (not exposed publicly). Use Redis AUTH in production.
- **Logs**: Log lines are stored in Redis with the same retention as the job. Structured logging omits prompt/result content (only metadata is logged).
- **Output files**: Written to a directory with appropriate permissions (0600).
- **Webhooks**: Use HTTPS for webhook URLs. Results are POST'ed to the configured URL.

### 4.2 What's Stored in Redis

| Data | Contains Sensitive Info? | Retention |
|------|------------------------|-----------|
| Job prompt | Possibly | Until job deleted |
| Job result | Likely | Until job deleted |
| Job logs | Possibly | Until job deleted |
| Skill content | Usually not | Until skill deleted |
| Cron definitions | Prompt text | Until cron deleted |
| Stats counters | No | Permanent |
| Worker heartbeats | No | 30s TTL |

### 4.3 Authentication / OAuth Token Handling

Claude Code authenticates via OAuth. Tokens are stored in `~/.claude/` on the host. For Docker deployments, this directory is bind-mounted read-only into the worker container.

- OAuth tokens are **never** stored in Redis
- OAuth tokens are **never** included in log output or API responses
- The worker container mounts `~/.claude/:ro` to prevent token modification
- If tokens expire, re-run `claude` on the host to refresh the OAuth session
- For API key auth (alternative): set `ANTHROPIC_API_KEY` env var on the worker — Claude Code will use it instead of OAuth

## 5. Reliability

### 5.1 Failure Recovery Matrix

| Component | Failure | Detection | Automatic Recovery |
|-----------|---------|-----------|-------------------|
| Redis | Crash | All services: connection error | Services retry with backoff. Redis recovers from AOF. |
| API server | Crash | Docker: health check fails | Docker restarts. Workers + scheduler unaffected. |
| Worker | Crash | Heartbeat expires (30s) | Reaper re-queues in-flight jobs. Docker restarts worker. |
| Worker | OOM | Heartbeat expires | Same as crash |
| claude -p | Crash | Non-zero exit code | Job marked failed. Retry up to 3x for worker crashes. |
| claude -p | Hang | Timeout (configurable) | Process killed. Job marked failed. |
| Scheduler | Crash | Docker: restart policy | Docker restarts. Missed cron ticks are lost (not queued retroactively). |
| Network | Redis disconnected | Connection pool error | All services retry with backoff |

### 5.2 Data Durability

Redis with AOF (`appendonly yes`, `appendfsync everysec`):
- Worst case data loss: ~1 second of writes
- On restart, Redis replays the AOF to restore state
- Job results stored on filesystem (via file output) provide additional durability

**What survives a Redis crash**:
- All jobs submitted more than ~1 second before the crash
- All completed job results (if using file output)
- All skill definitions
- All cron schedules

**What may be lost**:
- Jobs submitted in the last ~1 second
- In-flight job state transitions (recovered by reaper)
- Live log lines (non-critical, results are still captured)

### 5.3 Ordering Guarantees

- **Job execution order**: Within a priority level, FIFO (Redis list semantics). Higher priority jobs are always claimed before lower priority ones.
- **Log line order**: Preserved per-job (Redis RPUSH to a list). Cross-job ordering is not guaranteed.
- **Job state events**: Redis Streams preserve ordering and are resumable. The API server reads from `claw:stream:jobs` with a consumer group, so events are never missed even after reconnection.
- **Log line events**: Pub/Sub is fire-and-forget. Log lines may be lost during API reconnection, but this is acceptable — persistent logs in Redis lists are the source of truth.

### 5.3.1 UI Stale State Prevention

To prevent the UI from showing stale data after missed events:
1. **Primary**: Job state events use Redis Streams (`XREAD` with consumer groups), which are persistent and resumable
2. **Fallback**: The Flutter UI polls `GET /api/v1/jobs` every 30 seconds as a safety net
3. **On WebSocket reconnect**: The UI immediately refreshes all visible data via REST calls

### 5.4 At-Least-Once Delivery

Jobs are guaranteed to execute at least once (assuming workers are running):
- A submitted job stays in the pending queue until claimed
- If a worker dies, the reaper re-queues the job
- Up to 3 retries for worker failures

**Not exactly-once**: If a worker completes a job but dies before marking it complete, the reaper may re-queue it. The job would execute twice. For idempotent tasks (code reviews, analysis), this is harmless. For non-idempotent tasks (deployments, notifications), the `retry_count` field can be checked in the prompt to adjust behavior.

### 5.5 Circuit Breaking

If Redis is unreachable, services behave as follows:

| Service | Behavior |
|---------|----------|
| API | Returns 503 for all endpoints. WebSocket connections are dropped. |
| Worker | Stops claiming jobs. Retries Redis connection with exponential backoff (1s, 2s, 4s, 8s, 16s, 32s max). In-flight jobs continue executing (output stored locally, flushed to Redis on reconnect). |
| Scheduler | Stops submitting jobs. Cron ticks during outage are lost. File watcher pauses. |
| CLI | Returns error immediately with message to check API server connectivity. |

## 6. Authentication (Future — Phase 7)

The initial deployment assumes a trusted network (local machine or private network). Phase 7 adds:

### 6.1 API Authentication

```
Authorization: Bearer <api-key>
```

API keys stored in Redis, managed via CLI:

```bash
claw auth create-key --name "CI Pipeline" --scope jobs:write,skills:read
claw auth list-keys
claw auth revoke-key <key-id>
```

### 6.2 Scopes

| Scope | Allows |
|-------|--------|
| `jobs:read` | List and view jobs, results, logs |
| `jobs:write` | Submit, cancel, delete jobs |
| `skills:read` | List and view skills |
| `skills:write` | Create, update, delete skills |
| `crons:read` | List and view cron schedules |
| `crons:write` | Create, update, delete, trigger crons |
| `admin` | All of the above + worker status + system config |

### 6.3 Webhook Verification

GitHub webhooks verified via HMAC-SHA256:

```rust
fn verify_github_signature(
    secret: &str,
    payload: &[u8],
    signature_header: &str,
) -> bool {
    let expected = format!("sha256={}", hmac_sha256(secret, payload));
    constant_time_eq(expected.as_bytes(), signature_header.as_bytes())
}
```

## 7. Rate Limiting (Future — Phase 7)

API rate limits to prevent abuse:

| Endpoint | Limit |
|----------|-------|
| `POST /api/v1/jobs` | 60/minute |
| `GET /api/v1/*` | 300/minute |
| `POST /api/v1/webhook/*` | 120/minute |
| WebSocket messages | 30/second |

Implemented via tower middleware with Redis-backed counters.
