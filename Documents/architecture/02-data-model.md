# Data Model — Redis Schema and Job Lifecycle

## 1. Redis Key Namespace

All keys use the `claw:` prefix to avoid collisions with other Redis users. The key hierarchy is:

```
claw:
├── job:{uuid}                    # Hash   — job metadata
├── job:{uuid}:result             # String — JSON result blob
├── job:{uuid}:log                # List   — ordered log lines
│
├── queue:
│   ├── pending:{0-9}             # List   — priority queues (9=highest)
│   ├── running                   # Set    — currently executing job IDs
│   ├── completed                 # ZSet   — scored by completion timestamp
│   └── failed                    # ZSet   — scored by failure timestamp
│
├── worker:{id}:
│   ├── heartbeat                 # String — epoch timestamp, TTL 30s
│   └── current                   # String — job ID being executed
│
├── skill:{slug}                  # Hash   — skill metadata + content
├── skills:index                  # Set    — all skill IDs
│
├── cron:{uuid}                   # Hash   — cron schedule definition
├── crons:index                   # Set    — all cron IDs
│
├── stream:
│   └── jobs                      # Stream — job state transitions (reliable, resumable via XREAD)
│
├── events:
│   └── logs:{uuid}               # PubSub — live log lines per job (fire-and-forget, loss acceptable)
│
├── reaper:
│   └── leader                    # String — leader lease for reaper (SETNX with TTL)
│
└── stats:
    ├── total_submitted           # String (int)   — INCR
    ├── total_completed           # String (int)   — INCR
    ├── total_failed              # String (int)   — INCR
    ├── total_cost_usd            # String (float) — INCRBYFLOAT
    └── daily:{YYYY-MM-DD}:       # Per-day counters (with 30-day TTL)
        ├── submitted             # String (int)
        ├── completed             # String (int)
        ├── failed                # String (int)
        └── cost_usd              # String (float)
```

## 2. Job Data Model

### 2.1 Job Hash (`claw:job:{uuid}`)

| Field | Type | Description | Set By |
|-------|------|-------------|--------|
| `id` | UUID string | Unique job identifier | Submitter |
| `status` | Enum string | `pending`, `running`, `completed`, `failed`, `cancelled` | System |
| `prompt` | String | The raw user prompt text | Submitter |
| `skill_ids` | JSON array | Skill IDs to inject: `["code-review","rust"]` | Submitter |
| `skill_tags` | JSON array | Skill tags for auto-matching: `["rust"]` | Submitter |
| `working_dir` | Path string | Absolute path for `claude -p` cwd | Submitter |
| `model` | String (optional) | Claude model override: `"sonnet"`, `"opus"`, `"haiku"` | Submitter |
| `max_budget_usd` | Float (optional) | Maximum USD spend for this job | Submitter |
| `allowed_tools` | JSON array (optional) | Tool restrictions for claude CLI | Submitter |
| `output_dest` | JSON object | Where to send results (see 2.2) | Submitter |
| `source` | Enum string | `cli`, `api`, `cron`, `filewatcher` | System |
| `priority` | Integer 0-9 | Queue priority (default 5, 9=highest) | Submitter |
| `tags` | JSON array | Arbitrary tags for filtering: `["pr-review","urgent"]` | Submitter |
| `created_at` | ISO 8601 | When the job was submitted | System |
| `started_at` | ISO 8601 (optional) | When a worker claimed it | Worker |
| `completed_at` | ISO 8601 (optional) | When execution finished | Worker |
| `worker_id` | String (optional) | ID of the worker that claimed this job | Worker |
| `error` | String (optional) | Error message if status=failed | Worker |
| `cost_usd` | Float (optional) | Actual USD cost from claude output | Worker |
| `duration_ms` | Integer (optional) | Execution duration in milliseconds | Worker |
| `retry_count` | Integer | Number of times this job has been re-queued (default 0) | Reaper |
| `timeout_secs` | Integer (optional) | Per-job timeout override | Submitter |
| `cron_id` | UUID (optional) | If spawned by a cron schedule, the cron's ID | Scheduler |
| `skill_snapshot` | JSON object (optional) | Snapshot of resolved skill content at execution time (for reproducibility) | Worker |
| `assembled_prompt` | String (optional) | The full assembled prompt sent to claude (skills + metadata + user prompt) | Worker |

### 2.2 Output Destination (`output_dest` field)

Three variants, serialized as tagged JSON:

```json
// Store in Redis (default)
{"type": "redis"}

// Write to filesystem
{"type": "file", "path": "/output"}

// POST to webhook URL
{"type": "webhook", "url": "https://hooks.slack.com/services/..."}
```

When `type: "file"`, the worker writes to `{path}/{job_id}.json`.
When `type: "webhook"`, the worker POSTs a JSON payload to the URL.
Result is always also stored in Redis regardless of output_dest (for queryability).

### 2.3 Rust Type Definition

```rust
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Job {
    pub id: Uuid,
    pub status: JobStatus,
    pub prompt: String,
    #[serde(default)]
    pub skill_ids: Vec<String>,
    #[serde(default)]
    pub skill_tags: Vec<String>,
    pub working_dir: PathBuf,
    pub model: Option<String>,
    pub max_budget_usd: Option<f64>,
    pub allowed_tools: Option<Vec<String>>,
    #[serde(default = "OutputDest::default")]
    pub output_dest: OutputDest,
    pub source: JobSource,
    #[serde(default = "default_priority")]
    pub priority: u8,
    #[serde(default)]
    pub tags: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub worker_id: Option<String>,
    pub error: Option<String>,
    pub cost_usd: Option<f64>,
    pub duration_ms: Option<u64>,
    #[serde(default)]
    pub retry_count: u32,
    pub timeout_secs: Option<u64>,
    pub cron_id: Option<Uuid>,
    pub skill_snapshot: Option<serde_json::Value>,
    pub assembled_prompt: Option<String>,
}

fn default_priority() -> u8 { 5 }

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[derive(strum::Display, strum::EnumString)]
#[strum(serialize_all = "lowercase")]
pub enum JobStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum OutputDest {
    Redis,
    File { path: PathBuf },
    Webhook { url: String },
}

impl Default for OutputDest {
    fn default() -> Self { Self::Redis }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[derive(strum::Display, strum::EnumString)]
#[strum(serialize_all = "lowercase")]
pub enum JobSource {
    Cli,
    Api,
    Cron,
    FileWatcher,
}
```

## 3. Job Lifecycle State Machine

```
                  ┌─────────────────────────────────────────────┐
                  │              Job State Machine               │
                  │                                             │
                  │         submit()                            │
                  │            │                                │
                  │            ▼                                │
                  │      ┌──────────┐                          │
                  │      │ PENDING  │                          │
                  │      └────┬─────┘                          │
                  │           │                                │
                  │     ┌─────┴──────┐                         │
                  │     │            │                         │
                  │  claim()    cancel()                       │
                  │     │            │                         │
                  │     ▼            ▼                         │
                  │ ┌─────────┐  ┌───────────┐                │
                  │ │ RUNNING │  │ CANCELLED │ (terminal)     │
                  │ └────┬────┘  └───────────┘                │
                  │      │                                     │
                  │  ┌───┴────┐                                │
                  │  │        │                                │
                  │ ok()   fail()                              │
                  │  │        │                                │
                  │  │    ┌───┴──────────────┐                 │
                  │  │    │ retry_count < 3? │                 │
                  │  │    └───┬──────────┬───┘                 │
                  │  │       yes         no                    │
                  │  │        │          │                     │
                  │  │    requeue()      │                     │
                  │  │     → PENDING     │                     │
                  │  │                   │                     │
                  │  ▼                   ▼                     │
                  │ ┌───────────┐  ┌──────────┐               │
                  │ │ COMPLETED │  │  FAILED  │ (terminal)    │
                  │ │ (terminal)│  └──────────┘               │
                  │ └───────────┘                              │
                  │                                             │
                  └─────────────────────────────────────────────┘
```

### 3.1 State Transitions

| Transition | Trigger | Redis Operations |
|-----------|---------|-----------------|
| → PENDING | `submit()` | `HSET job:{id}`, `RPUSH queue:pending:{priority}`, `PUBLISH events:jobs` |
| PENDING → RUNNING | `claim()` (Lua) | `LPOP queue:pending:{p}`, `SADD queue:running`, `HSET status=running, started_at, worker_id` |
| PENDING → CANCELLED | `cancel()` | `LREM queue:pending:{p}`, `HSET status=cancelled` |
| RUNNING → COMPLETED | `complete()` | `SREM queue:running`, `ZADD queue:completed`, `HSET status=completed, completed_at, cost_usd, duration_ms`, `SET job:{id}:result` |
| RUNNING → FAILED | `fail()` | `SREM queue:running`, `ZADD queue:failed`, `HSET status=failed, error` |
| RUNNING → PENDING | `requeue()` (reaper) | `SREM queue:running`, `HINCRBY retry_count`, `RPUSH queue:pending:5`, `HSET status=pending` |
| RUNNING → CANCELLED | `cancel()` | Kill subprocess, `SREM queue:running`, `HSET status=cancelled` |

### 3.2 Atomic Job Claim — Lua Script

This is the most critical Redis operation. It must be atomic to prevent two workers from claiming the same job.

```lua
-- claim_job.lua
--
-- Atomically claims the highest-priority pending job.
-- Iterates priority queues from 9 (highest) down to 0 (lowest).
--
-- KEYS: (none — keys are constructed dynamically)
-- ARGV[1] = worker_id
-- ARGV[2] = current ISO 8601 timestamp
--
-- Returns: job_id string, or nil if no jobs available

for priority = 9, 0, -1 do
    local queue_key = 'claw:queue:pending:' .. priority
    local job_id = redis.call('LPOP', queue_key)
    if job_id then
        -- Move to running set
        redis.call('SADD', 'claw:queue:running', job_id)

        -- Update job metadata
        local job_key = 'claw:job:' .. job_id
        redis.call('HSET', job_key,
            'status', 'running',
            'started_at', ARGV[2],
            'worker_id', ARGV[1]
        )

        -- Append state change to job events stream (reliable, resumable)
        redis.call('XADD', 'claw:stream:jobs', '*',
            'type', 'job_update',
            'job_id', job_id,
            'status', 'running',
            'worker_id', ARGV[1],
            'timestamp', ARGV[2]
        )

        return job_id
    end
end

return nil
```

**Why Lua?** Redis executes Lua scripts atomically — no other command can interleave. This guarantees exactly-once job claiming without external locking.

**Why Streams instead of Pub/Sub for job events?** Redis Pub/Sub is fire-and-forget — if the API server disconnects momentarily, events are permanently lost and the UI shows stale state. Redis Streams persist messages and support consumer groups with `XREAD BLOCK`, allowing the API server to resume from the last-read position after reconnection. Pub/Sub is still used for log lines where occasional loss is acceptable.

**Redis Cluster limitation**: These Lua scripts construct key names dynamically inside the script. Redis Cluster requires all accessed keys to be declared in the `KEYS` array. These scripts will not work with Redis Cluster — this is acceptable for the single-instance deployment target, but should be documented.

### 3.3 Job Completion — Lua Script

```lua
-- complete_job.lua
--
-- ARGV[1] = job_id
-- ARGV[2] = timestamp (ISO 8601)
-- ARGV[3] = cost_usd (string float)
-- ARGV[4] = duration_ms (string int)
-- ARGV[5] = result_json (string)

local job_key = 'claw:job:' .. ARGV[1]

-- Update job hash
redis.call('HSET', job_key,
    'status', 'completed',
    'completed_at', ARGV[2],
    'cost_usd', ARGV[3],
    'duration_ms', ARGV[4]
)

-- Move from running to completed
redis.call('SREM', 'claw:queue:running', ARGV[1])
redis.call('ZADD', 'claw:queue:completed', tonumber(ARGV[2]) or 0, ARGV[1])

-- Store result
redis.call('SET', 'claw:job:' .. ARGV[1] .. ':result', ARGV[5])

-- Update stats (total + daily)
redis.call('INCR', 'claw:stats:total_completed')
redis.call('INCRBYFLOAT', 'claw:stats:total_cost_usd', ARGV[3])
local date_key = os.date('%Y-%m-%d')
redis.call('INCR', 'claw:stats:daily:' .. date_key .. ':completed')
redis.call('INCRBYFLOAT', 'claw:stats:daily:' .. date_key .. ':cost_usd', ARGV[3])
-- Set 30-day TTL on daily keys (idempotent)
redis.call('EXPIRE', 'claw:stats:daily:' .. date_key .. ':completed', 2592000)
redis.call('EXPIRE', 'claw:stats:daily:' .. date_key .. ':cost_usd', 2592000)

-- Append to job events stream (reliable, resumable)
redis.call('XADD', 'claw:stream:jobs', '*',
    'type', 'job_update',
    'job_id', ARGV[1],
    'status', 'completed',
    'timestamp', ARGV[2]
)

return 'OK'
```

### 3.4 Atomic Reaper — Lua Script

The reaper must re-queue dead-worker jobs atomically to prevent duplicate re-queuing when multiple worker instances run reapers.

```lua
-- reaper_requeue.lua
--
-- Atomically checks if a worker is dead and re-queues its job.
-- Returns 1 if re-queued, 0 if worker is alive, -1 if max retries exceeded.
--
-- ARGV[1] = job_id
-- ARGV[2] = worker_id
-- ARGV[3] = max_retries (e.g., "3")
-- ARGV[4] = timestamp

-- Check if heartbeat still exists
local hb_key = 'claw:worker:' .. ARGV[2] .. ':heartbeat'
if redis.call('EXISTS', hb_key) == 1 then
    return 0  -- Worker is alive
end

-- Verify job is still in running set (another reaper may have handled it)
if redis.call('SISMEMBER', 'claw:queue:running', ARGV[1]) == 0 then
    return 0  -- Already handled
end

local job_key = 'claw:job:' .. ARGV[1]
local retry_count = tonumber(redis.call('HGET', job_key, 'retry_count') or '0')

if retry_count < tonumber(ARGV[3]) then
    -- Re-queue
    redis.call('SREM', 'claw:queue:running', ARGV[1])
    redis.call('HSET', job_key,
        'status', 'pending',
        'retry_count', retry_count + 1,
        'worker_id', '',
        'started_at', ''
    )
    redis.call('RPUSH', 'claw:queue:pending:5', ARGV[1])
    redis.call('XADD', 'claw:stream:jobs', '*',
        'type', 'job_update', 'job_id', ARGV[1],
        'status', 'pending', 'timestamp', ARGV[4]
    )
    return 1
else
    -- Max retries exceeded
    redis.call('SREM', 'claw:queue:running', ARGV[1])
    redis.call('ZADD', 'claw:queue:failed', tonumber(ARGV[4]) or 0, ARGV[1])
    redis.call('HSET', job_key,
        'status', 'failed',
        'error', 'Worker died and max retries (' .. ARGV[3] .. ') exceeded'
    )
    redis.call('XADD', 'claw:stream:jobs', '*',
        'type', 'job_update', 'job_id', ARGV[1],
        'status', 'failed', 'timestamp', ARGV[4]
    )
    return -1
end
```

The reaper is also gated behind a leader lease:
```
SETNX claw:reaper:leader {worker_id}
EXPIRE claw:reaper:leader 20
```
Only the worker that holds the lease runs the reaper scan. The lease expires automatically if the leader dies.

## 4. Skill Data Model

### 4.1 Skill Hash (`claw:skill:{slug}`)

| Field | Type | Description |
|-------|------|-------------|
| `id` | Slug string | URL-safe identifier: `"code-review"`, `"rust-lint"` |
| `name` | String | Human-readable: `"Code Review Template"` |
| `skill_type` | Enum string | `"template"`, `"claude_config"`, `"script"` |
| `content` | String | The actual skill content (markdown, CLAUDE.md text, or script body) |
| `description` | String | Short description for UI display and search |
| `tags` | JSON array | Tags for filtering and auto-matching: `["rust","review"]` |
| `created_at` | ISO 8601 | When the skill was created |
| `updated_at` | ISO 8601 | When the skill was last modified |

### 4.2 Rust Type

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Skill {
    pub id: String,
    pub name: String,
    pub skill_type: SkillType,
    pub content: String,
    pub description: String,
    #[serde(default)]
    pub tags: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[derive(strum::Display, strum::EnumString)]
#[strum(serialize_all = "snake_case")]
pub enum SkillType {
    Template,
    ClaudeConfig,
    Script,
}
```

### 4.3 Skill Examples

**Template skill** (`code-review`):
```json
{
    "id": "code-review",
    "name": "Code Review",
    "skill_type": "template",
    "description": "Structured code review guidelines",
    "tags": ["review", "quality"],
    "content": "When reviewing code, evaluate:\n1. Correctness...\n2. Security...\n3. Performance..."
}
```

**CLAUDE.md config skill** (`rust-project`):
```json
{
    "id": "rust-project",
    "name": "Rust Project Config",
    "skill_type": "claude_config",
    "description": "CLAUDE.md for Rust projects - sets conventions",
    "tags": ["rust", "config"],
    "content": "# Project Conventions\n- Use thiserror for errors\n- Prefer &str over String in function params\n..."
}
```

**Script skill** (`run-tests`):
```json
{
    "id": "run-tests",
    "name": "Test Runner",
    "skill_type": "script",
    "description": "Script that runs tests and reports results",
    "tags": ["testing"],
    "content": "#!/bin/bash\nset -euo pipefail\ncargo test --workspace 2>&1"
}
```

## 5. Cron Schedule Data Model

### 5.1 Cron Hash (`claw:cron:{uuid}`)

| Field | Type | Description |
|-------|------|-------------|
| `id` | UUID string | Unique cron identifier |
| `name` | String | Human-readable name: `"Morning PR Review"` |
| `schedule` | Cron string | Standard cron expression: `"0 9 * * MON-FRI"` |
| `enabled` | Boolean | Whether this schedule is active |
| `prompt` | String | The prompt to submit when triggered |
| `skill_ids` | JSON array | Skills to attach to generated jobs |
| `working_dir` | Path string | Working directory for generated jobs |
| `model` | String (optional) | Model override |
| `max_budget_usd` | Float (optional) | Budget per generated job |
| `output_dest` | JSON object | Output destination for generated jobs |
| `tags` | JSON array | Tags to apply to generated jobs |
| `priority` | Integer 0-9 | Priority for generated jobs |
| `last_run` | ISO 8601 (optional) | When this cron last fired |
| `last_job_id` | UUID (optional) | Job ID from the most recent trigger (for deduplication) |
| `next_run` | ISO 8601 (optional) | Next scheduled fire time |
| `created_at` | ISO 8601 | When the cron was created |

### 5.2 Rust Type

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronSchedule {
    pub id: Uuid,
    pub name: String,
    pub schedule: String,           // cron expression
    pub enabled: bool,
    pub prompt: String,
    #[serde(default)]
    pub skill_ids: Vec<String>,
    pub working_dir: PathBuf,
    pub model: Option<String>,
    pub max_budget_usd: Option<f64>,
    #[serde(default = "OutputDest::default")]
    pub output_dest: OutputDest,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default = "default_priority")]
    pub priority: u8,
    pub last_run: Option<DateTime<Utc>>,
    pub next_run: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}
```

## 6. WebSocket Event Model

### 6.1 Client → Server Messages

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WsClientMessage {
    Subscribe { channel: WsChannel },
    Unsubscribe { channel: WsChannel },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WsChannel {
    Jobs,
    JobLogs { job_id: Uuid },
    Stats,
}
```

### 6.2 Server → Client Messages

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WsServerMessage {
    JobUpdate {
        job_id: Uuid,
        status: JobStatus,
        worker_id: Option<String>,
        timestamp: DateTime<Utc>,
    },
    JobLog {
        job_id: Uuid,
        line: String,
        timestamp: DateTime<Utc>,
    },
    Stats {
        pending: u64,
        running: u64,
        completed_today: u64,
        failed_today: u64,
        total_cost_today: f64,
    },
    Error {
        message: String,
    },
}
```

## 7. Job File Format (`.job`)

For the file watcher ingestion method, jobs are defined as JSON files:

```json
{
    "prompt": "Review all open PRs in the repo and summarize findings",
    "skill_ids": ["code-review", "rust-review"],
    "skill_tags": [],
    "working_dir": "/repos/my-project",
    "model": "sonnet",
    "max_budget_usd": 2.00,
    "output": {"type": "file", "path": "/output"},
    "tags": ["automated", "pr-review"],
    "priority": 7,
    "timeout_secs": 900
}
```

All fields except `prompt` are optional and fall back to configured defaults.

**Atomic write convention**: To prevent the file watcher from reading a partially-written file, submitters should write to a `.job.tmp` file first, then rename to `.job`. File rename is atomic on POSIX systems. The watcher only processes files matching `*.job` and ignores `*.tmp` files. This convention should be documented for all file-based job submission.

## 8. Data Retention and Cleanup

### 8.1 Automatic Cleanup

| Data | Retention | Mechanism |
|------|----------|-----------|
| Completed job hashes | 7 days (configurable) | Background cleanup task scans `queue:completed` |
| Failed job hashes | 14 days (configurable) | Background cleanup task scans `queue:failed` |
| Job logs | Same as parent job | Deleted when job hash is deleted |
| Job results | Same as parent job | Deleted when job hash is deleted |
| Worker heartbeats | 30 seconds | Redis TTL auto-expiry |
| Stats counters | Permanent | Never auto-deleted |

### 8.2 Manual Cleanup

The API and CLI support explicit deletion:
- `DELETE /api/v1/jobs/{id}` — removes job hash, result, logs, and sorted set entry
- `claw delete <job_id>` — same via CLI

### 8.3 Redis Memory Considerations

Estimated per-job memory footprint:
- Job hash: ~1 KB (metadata)
- Job result: 1-50 KB (varies by output)
- Job logs: 5-500 KB (varies by execution length)
- **Total per job: ~10-550 KB**

At 100 jobs/day with 7-day retention: ~700 jobs × ~100 KB average = **~70 MB**. Well within single-instance Redis capacity.

For heavier usage, configure shorter retention or move completed results to filesystem storage.
