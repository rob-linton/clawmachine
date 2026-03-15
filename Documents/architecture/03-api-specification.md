# API Specification — REST and WebSocket

## 1. Base URL and Versioning

```
Base URL:    http://{host}:8080/api/v1
WebSocket:   ws://{host}:8080/api/v1/ws
Static UI:   http://{host}:8080/          (Flutter web build)
```

API is versioned via URL path (`/v1`). Breaking changes increment the version. Non-breaking additions (new optional fields, new endpoints) do not.

## 2. Common Patterns

### 2.1 Response Format

All responses are JSON. Successful responses:

```json
// Single resource
{
    "id": "f47ac10b-58cc-4372-a567-0e02b2c3d479",
    "status": "pending",
    ...
}

// Collection
{
    "items": [...],
    "total": 42,
    "offset": 0,
    "limit": 20
}
```

Error responses:

```json
{
    "error": {
        "code": "not_found",
        "message": "Job f47ac10b not found"
    }
}
```

### 2.2 HTTP Status Codes

| Code | Usage |
|------|-------|
| 200 | Success (GET, PUT) |
| 201 | Created (POST that creates a resource) |
| 204 | No Content (DELETE) |
| 400 | Bad Request (invalid JSON, missing required field) |
| 404 | Not Found |
| 409 | Conflict (e.g., cancelling an already-completed job) |
| 422 | Unprocessable Entity (valid JSON but semantic error, e.g., invalid cron expression) |
| 500 | Internal Server Error |

### 2.3 Pagination

Collection endpoints support:
- `?limit=N` — max items to return (default 20, max 100)
- `?offset=N` — skip first N items (default 0)

Response includes `total` for UI pagination controls.

### 2.4 Filtering

Collection endpoints support query parameters for filtering. Multiple filters are ANDed.

## 3. Jobs API

### 3.1 Submit Job

```
POST /api/v1/jobs
Content-Type: application/json
```

**Request body:**

```json
{
    "prompt": "Review the code in src/main.rs for security issues",
    "skill_ids": ["code-review", "security-audit"],
    "skill_tags": ["rust"],
    "working_dir": "/repos/my-project",
    "model": "sonnet",
    "max_budget_usd": 1.50,
    "allowed_tools": ["Read", "Grep", "Glob"],
    "output": {"type": "webhook", "url": "https://hooks.slack.com/..."},
    "tags": ["security", "automated"],
    "priority": 8,
    "timeout_secs": 600
}
```

| Field | Required | Default | Description |
|-------|----------|---------|-------------|
| `prompt` | Yes | — | The task prompt for Claude |
| `skill_ids` | No | `[]` | Skill IDs to inject |
| `skill_tags` | No | `[]` | Tags for auto-matching skills |
| `working_dir` | No | config default | Working directory for claude |
| `model` | No | config default | Model override |
| `max_budget_usd` | No | config default | Max spend |
| `allowed_tools` | No | all | Tool restrictions |
| `output` | No | `{"type":"redis"}` | Output destination |
| `tags` | No | `[]` | Arbitrary tags |
| `priority` | No | `5` | 0-9 (9=highest) |
| `timeout_secs` | No | `1800` | Execution timeout |

**Response (201):**

```json
{
    "id": "f47ac10b-58cc-4372-a567-0e02b2c3d479",
    "status": "pending",
    "priority": 8,
    "created_at": "2026-03-15T22:30:00Z"
}
```

### 3.2 List Jobs

```
GET /api/v1/jobs?status=running&tag=security&limit=20&offset=0&sort=newest
```

| Parameter | Type | Default | Options |
|-----------|------|---------|---------|
| `status` | string | all | `pending`, `running`, `completed`, `failed`, `cancelled` |
| `tag` | string | all | Filter by tag (repeatable: `?tag=a&tag=b` = has both) |
| `source` | string | all | `cli`, `api`, `cron`, `filewatcher` |
| `limit` | int | 20 | 1-100 |
| `offset` | int | 0 | |
| `sort` | string | `newest` | `newest`, `oldest`, `priority` |
| `search` | string | — | Substring search in prompt text |

**Response (200):**

```json
{
    "items": [
        {
            "id": "f47ac10b-58cc-4372-a567-0e02b2c3d479",
            "status": "running",
            "prompt": "Review the code in src/main.rs for security issues",
            "source": "api",
            "priority": 8,
            "tags": ["security", "automated"],
            "model": "sonnet",
            "worker_id": "worker-1-task-0",
            "created_at": "2026-03-15T22:30:00Z",
            "started_at": "2026-03-15T22:30:02Z",
            "cost_usd": null,
            "duration_ms": null
        }
    ],
    "total": 1,
    "offset": 0,
    "limit": 20
}
```

### 3.3 Get Job Detail

```
GET /api/v1/jobs/{id}
```

**Response (200):** Full job object with all fields from the hash.

```json
{
    "id": "f47ac10b-58cc-4372-a567-0e02b2c3d479",
    "status": "completed",
    "prompt": "Review the code in src/main.rs for security issues",
    "skill_ids": ["code-review", "security-audit"],
    "skill_tags": ["rust"],
    "working_dir": "/repos/my-project",
    "model": "sonnet",
    "max_budget_usd": 1.50,
    "allowed_tools": ["Read", "Grep", "Glob"],
    "output_dest": {"type": "webhook", "url": "https://hooks.slack.com/..."},
    "source": "api",
    "priority": 8,
    "tags": ["security", "automated"],
    "created_at": "2026-03-15T22:30:00Z",
    "started_at": "2026-03-15T22:30:02Z",
    "completed_at": "2026-03-15T22:33:15Z",
    "worker_id": "worker-1-task-0",
    "error": null,
    "cost_usd": 0.42,
    "duration_ms": 193000,
    "retry_count": 0,
    "timeout_secs": 600,
    "cron_id": null
}
```

### 3.4 Get Job Result

```
GET /api/v1/jobs/{id}/result
```

**Response (200):**

```json
{
    "job_id": "f47ac10b-58cc-4372-a567-0e02b2c3d479",
    "result": "## Security Review\n\nI found 3 issues in src/main.rs:\n\n1. **SQL injection** on line 42...",
    "cost_usd": 0.42,
    "duration_ms": 193000,
    "model": "sonnet",
    "completed_at": "2026-03-15T22:33:15Z"
}
```

**Response (404):** Job not found or not yet completed.

### 3.5 Get Job Logs

```
GET /api/v1/jobs/{id}/logs?offset=0&limit=500
```

**Response (200):**

```json
{
    "job_id": "f47ac10b-58cc-4372-a567-0e02b2c3d479",
    "lines": [
        {"index": 0, "content": "{\"type\":\"assistant\",\"message\":\"I'll review...\"}"},
        {"index": 1, "content": "{\"type\":\"tool_use\",\"tool\":\"Read\",\"input\":{\"file_path\":\"src/main.rs\"}}"},
        {"index": 2, "content": "{\"type\":\"tool_result\",\"output\":\"...\"}"}
    ],
    "total": 47,
    "offset": 0,
    "limit": 500
}
```

### 3.6 Cancel Job

```
POST /api/v1/jobs/{id}/cancel
```

For pending jobs: removes from queue, sets status=cancelled.
For running jobs: signals the worker to kill the subprocess, sets status=cancelled.

**Response (200):**

```json
{
    "id": "f47ac10b-58cc-4372-a567-0e02b2c3d479",
    "status": "cancelled"
}
```

**Response (409):** Job is already completed/failed/cancelled.

### 3.7 Delete Job

```
DELETE /api/v1/jobs/{id}
```

Deletes job hash, result, logs, and queue entries. Only allowed for terminal states (completed, failed, cancelled).

**Response (204):** No content.
**Response (409):** Job is still pending or running.

## 4. Skills API

### 4.1 Create Skill

```
POST /api/v1/skills
Content-Type: application/json
```

**Request body:**

```json
{
    "id": "rust-security",
    "name": "Rust Security Review",
    "skill_type": "template",
    "content": "When reviewing Rust code for security:\n1. Check for unsafe blocks...\n2. Verify input validation...",
    "description": "Security-focused code review template for Rust projects",
    "tags": ["rust", "security", "review"]
}
```

| Field | Required | Description |
|-------|----------|-------------|
| `id` | Yes | URL-safe slug (lowercase, hyphens) |
| `name` | Yes | Display name |
| `skill_type` | Yes | `template`, `claude_config`, or `script` |
| `content` | Yes | The skill content |
| `description` | No | Short description |
| `tags` | No | Array of tags |

**Response (201):** Full skill object.

### 4.2 List Skills

```
GET /api/v1/skills?type=template&tag=rust
```

| Parameter | Type | Default |
|-----------|------|---------|
| `type` | string | all |
| `tag` | string | all |
| `search` | string | — |

**Response (200):**

```json
{
    "items": [
        {
            "id": "rust-security",
            "name": "Rust Security Review",
            "skill_type": "template",
            "description": "Security-focused code review template for Rust projects",
            "tags": ["rust", "security", "review"],
            "created_at": "2026-03-10T10:00:00Z",
            "updated_at": "2026-03-10T10:00:00Z"
        }
    ],
    "total": 1
}
```

Note: `content` is omitted from list responses for brevity. Use the detail endpoint to fetch content.

### 4.3 Get Skill

```
GET /api/v1/skills/{id}
```

**Response (200):** Full skill object including `content`.

### 4.4 Update Skill

```
PUT /api/v1/skills/{id}
Content-Type: application/json
```

**Request body:** Same as create. All fields are replaced (full update, not patch).

**Response (200):** Updated skill object.

### 4.5 Delete Skill

```
DELETE /api/v1/skills/{id}
```

**Response (204):** No content.

## 5. Crons API

### 5.1 Create Cron Schedule

```
POST /api/v1/crons
Content-Type: application/json
```

**Request body:**

```json
{
    "name": "Morning PR Review",
    "schedule": "0 9 * * MON-FRI",
    "enabled": true,
    "prompt": "Review all open PRs in the repository and post summaries",
    "skill_ids": ["code-review"],
    "working_dir": "/repos/main-project",
    "model": "sonnet",
    "max_budget_usd": 5.00,
    "output": {"type": "webhook", "url": "https://hooks.slack.com/..."},
    "tags": ["automated", "pr-review"],
    "priority": 6
}
```

| Field | Required | Default |
|-------|----------|---------|
| `name` | Yes | — |
| `schedule` | Yes | — |
| `enabled` | No | `true` |
| `prompt` | Yes | — |
| `skill_ids` | No | `[]` |
| `working_dir` | No | config default |
| `model` | No | config default |
| `max_budget_usd` | No | config default |
| `output` | No | `{"type":"redis"}` |
| `tags` | No | `[]` |
| `priority` | No | `5` |

**Response (201):** Full cron object with `id`, `next_run` calculated.

### 5.2 List Crons

```
GET /api/v1/crons
```

**Response (200):**

```json
{
    "items": [
        {
            "id": "a1b2c3d4-...",
            "name": "Morning PR Review",
            "schedule": "0 9 * * MON-FRI",
            "enabled": true,
            "prompt": "Review all open PRs...",
            "last_run": "2026-03-14T09:00:00Z",
            "next_run": "2026-03-16T09:00:00Z",
            "created_at": "2026-03-01T10:00:00Z"
        }
    ],
    "total": 1
}
```

### 5.3 Get, Update, Delete Cron

Same pattern as skills: `GET/PUT/DELETE /api/v1/crons/{id}`.

### 5.4 Trigger Cron Manually

```
POST /api/v1/crons/{id}/trigger
```

Immediately creates and submits a job using the cron's configuration, regardless of schedule or enabled state. The `last_run` is NOT updated (manual triggers don't affect the schedule).

**Response (201):**

```json
{
    "job_id": "new-job-uuid-...",
    "cron_id": "a1b2c3d4-...",
    "status": "pending"
}
```

## 6. Status API

### 6.1 System Status

```
GET /api/v1/status
```

**Response (200):**

```json
{
    "status": "healthy",
    "redis": "connected",
    "uptime_secs": 86400,
    "queue": {
        "pending": 5,
        "running": 2,
        "completed_today": 47,
        "failed_today": 3,
        "total_submitted": 1250,
        "total_completed": 1200,
        "total_failed": 50
    },
    "costs": {
        "today_usd": 12.34,
        "total_usd": 456.78
    },
    "workers": {
        "active": 2,
        "total_capacity": 4
    }
}
```

### 6.2 Workers Status

```
GET /api/v1/workers
```

**Response (200):**

```json
{
    "workers": [
        {
            "id": "worker-1-task-0",
            "status": "busy",
            "current_job_id": "f47ac10b-...",
            "current_job_prompt": "Review the code...",
            "last_heartbeat": "2026-03-15T22:35:10Z"
        },
        {
            "id": "worker-1-task-1",
            "status": "idle",
            "current_job_id": null,
            "current_job_prompt": null,
            "last_heartbeat": "2026-03-15T22:35:10Z"
        }
    ]
}
```

## 7. Webhook Endpoints

### 7.1 Generic Webhook Submit

```
POST /api/v1/webhook/submit
Content-Type: application/json
```

Same body as `POST /api/v1/jobs`. Exists as an alias with a webhook-friendly URL.

### 7.2 GitHub Webhook

```
POST /api/v1/webhook/github
Content-Type: application/json
X-GitHub-Event: pull_request
X-Hub-Signature-256: sha256=...
```

Handles GitHub webhook events and auto-creates jobs:

| GitHub Event | Action | Job Created |
|-------------|--------|-------------|
| `pull_request` | `opened`, `synchronize` | PR code review job |
| `issues` | `opened` | Issue analysis job |
| `push` | (to default branch) | Post-merge review job |

The webhook handler:
1. Validates the signature using a configured webhook secret
2. Parses the event type and payload
3. Constructs an appropriate prompt with PR/issue context
4. Submits a job with auto-assigned skills based on event type
5. Returns 202 Accepted

**Configuration** (environment variables):
```
CLAW_GITHUB_WEBHOOK_SECRET=whsec_...
CLAW_GITHUB_DEFAULT_SKILLS=code-review
CLAW_GITHUB_DEFAULT_MODEL=sonnet
CLAW_GITHUB_DEFAULT_BUDGET=2.00
```

**Response (202):**

```json
{
    "job_id": "f47ac10b-...",
    "event": "pull_request",
    "action": "opened",
    "ref": "org/repo#42"
}
```

## 8. WebSocket Protocol

### 8.1 Connection

```
GET /api/v1/ws → 101 Switching Protocols
```

After upgrade, the connection speaks JSON messages in both directions.

### 8.2 Client Messages

**Subscribe to job events:**
```json
{"type": "subscribe", "channel": "jobs"}
```

**Subscribe to logs for a specific job:**
```json
{"type": "subscribe", "channel": "job_logs", "job_id": "f47ac10b-..."}
```

**Subscribe to periodic stats:**
```json
{"type": "subscribe", "channel": "stats"}
```

**Unsubscribe:**
```json
{"type": "unsubscribe", "channel": "jobs"}
{"type": "unsubscribe", "channel": "job_logs", "job_id": "f47ac10b-..."}
```

### 8.3 Server Messages

**Job state change:**
```json
{
    "type": "job_update",
    "job_id": "f47ac10b-...",
    "status": "running",
    "worker_id": "worker-1-task-0",
    "timestamp": "2026-03-15T22:30:02Z"
}
```

**Job log line (streamed in real-time during execution):**
```json
{
    "type": "job_log",
    "job_id": "f47ac10b-...",
    "line": "{\"type\":\"assistant\",\"message\":\"I'll start by reading...\"}",
    "timestamp": "2026-03-15T22:30:05Z"
}
```

**System stats (sent every 5 seconds to `stats` subscribers):**
```json
{
    "type": "stats",
    "pending": 3,
    "running": 2,
    "completed_today": 48,
    "failed_today": 3,
    "total_cost_today": 12.76
}
```

**Error (malformed subscription, etc.):**
```json
{
    "type": "error",
    "message": "Invalid channel: foobar"
}
```

### 8.4 Server-Side Implementation

The API server runs a dedicated Redis Pub/Sub listener task that:

1. Subscribes to `claw:events:jobs` and `claw:events:logs:*` using pattern subscription
2. Receives events and broadcasts them via `tokio::sync::broadcast`
3. Each WebSocket connection task listens on the broadcast channel
4. Filters events based on the client's active subscriptions
5. Serializes matching events to JSON and sends over the WebSocket

This means Redis Pub/Sub has exactly one subscriber (the API server), regardless of how many WebSocket clients are connected. The fan-out happens in-process via tokio broadcast.

### 8.5 Reconnection

The WebSocket protocol is stateless — clients must re-subscribe after reconnection. The Flutter UI should implement automatic reconnection with exponential backoff and re-subscribe on connect.

## 9. Axum Router Assembly

```rust
use axum::{Router, routing::{get, post, put, delete}};
use tower_http::{cors::CorsLayer, services::{ServeDir, ServeFile}};

pub fn build_router(state: AppState) -> Router {
    let api = Router::new()
        // Jobs
        .route("/jobs", post(routes::jobs::create).get(routes::jobs::list))
        .route("/jobs/{id}", get(routes::jobs::get_one).delete(routes::jobs::delete))
        .route("/jobs/{id}/result", get(routes::jobs::get_result))
        .route("/jobs/{id}/logs", get(routes::jobs::get_logs))
        .route("/jobs/{id}/cancel", post(routes::jobs::cancel))
        // Skills
        .route("/skills", post(routes::skills::create).get(routes::skills::list))
        .route("/skills/{id}",
            get(routes::skills::get_one)
            .put(routes::skills::update)
            .delete(routes::skills::delete))
        // Crons
        .route("/crons", post(routes::crons::create).get(routes::crons::list))
        .route("/crons/{id}",
            get(routes::crons::get_one)
            .put(routes::crons::update)
            .delete(routes::crons::delete))
        .route("/crons/{id}/trigger", post(routes::crons::trigger))
        // Status
        .route("/status", get(routes::status::health))
        .route("/workers", get(routes::status::workers))
        // Webhooks
        .route("/webhook/submit", post(routes::webhook::submit))
        .route("/webhook/github", post(routes::webhook::github))
        // WebSocket
        .route("/ws", get(ws::handler));

    Router::new()
        .nest("/api/v1", api)
        .fallback_service(
            ServeDir::new(&state.config.static_dir)
                .fallback(ServeFile::new(
                    format!("{}/index.html", &state.config.static_dir)
                ))
        )
        .layer(CorsLayer::permissive())  // tighten in production
        .with_state(state)
}
```
