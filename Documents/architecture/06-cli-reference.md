# CLI Reference — `claw` Command

## 1. Overview

The `claw` CLI is the primary interface for interacting with Claw Machine from the terminal. It communicates with the API server via REST endpoints, ensuring a single codepath for validation, defaults, and event publishing. The `--follow` flag uses the WebSocket endpoint for real-time log streaming.

**Binary name**: `claw` (installed as `claw` or `claw-cli`)

**Requires**: The API server (`claw-api`) must be running.

**Configuration**: `~/.claw/config.toml` (user) or `./claw.toml` (project) or environment variables.

## 2. Global Options

```
claw [OPTIONS] <COMMAND>

Options:
  --api-url <URL>      API server URL [env: CLAW_API_URL] [default: http://127.0.0.1:8080]
  --config <FILE>      Config file path [default: ~/.claw/config.toml]
  --json               Output in JSON format (where applicable)
  --quiet              Suppress non-essential output
  --verbose            Show debug-level output
  -h, --help           Print help
  -V, --version        Print version
```

## 3. Commands

### 3.1 `claw submit` — Submit a Job

```
claw submit [OPTIONS] <PROMPT>

Arguments:
  <PROMPT>  The task prompt for Claude (or "-" to read from stdin)

Options:
  -s, --skill <ID>           Skill IDs to inject (repeatable)
      --skill-tag <TAG>      Match skills by tag (repeatable)
  -d, --working-dir <PATH>   Working directory for claude [default: from config]
  -m, --model <MODEL>        Model override (sonnet, opus, haiku)
  -b, --budget <USD>         Max budget in USD [default: from config]
      --allowed-tools <T>    Restrict allowed tools (repeatable)
  -o, --output <DEST>        Output destination:
                              "redis" (default)
                              "file:/path/to/dir"
                              "webhook:https://url"
  -p, --priority <0-9>       Job priority [default: 5]
  -t, --tag <TAG>            Job tags for filtering (repeatable)
      --timeout <SECS>       Execution timeout in seconds [default: 1800]
  -w, --wait                 Block until job completes, print result
  -f, --follow               Stream job logs to terminal
```

**Examples**:

```bash
# Simple submission
claw submit "Review the code in src/main.rs"

# With skills and options
claw submit "Review this PR" \
    --skill code-review \
    --skill rust-conventions \
    --working-dir /repos/my-project \
    --model sonnet \
    --budget 2.00 \
    --output webhook:https://hooks.slack.com/xyz \
    --priority 8 \
    --tag pr-review

# Read prompt from stdin (pipe a file)
cat prompt.md | claw submit -

# Submit and wait for result
claw submit --wait "What files are in this directory?"

# Submit and stream logs
claw submit --follow "Refactor the database module" --working-dir /repos/app

# Submit with tool restrictions
claw submit "Only read and analyze, don't modify anything" \
    --allowed-tools Read --allowed-tools Grep --allowed-tools Glob
```

**Output (default)**:
```
Job submitted: f47ac10b-58cc-4372-a567-0e02b2c3d479
Status: pending
Priority: 8
```

**Output (--wait)**:
```
Job submitted: f47ac10b-58cc-4372-a567-0e02b2c3d479
Waiting for completion...
[============================] Completed in 2m 13s ($0.42)

## Review Summary
Found 3 issues in src/main.rs...
```

**Output (--follow)**:
```
Job submitted: f47ac10b-58cc-4372-a567-0e02b2c3d479
Streaming logs...
[22:30:02] assistant: I'll start by reading the file...
[22:30:03] tool_use: Read src/main.rs
[22:30:04] tool_result: (142 lines)
[22:30:07] assistant: I found several issues...
...
[22:32:15] Completed ($0.42, 2m 13s)
```

### 3.2 `claw status` — Queue Status / Job Detail

```
claw status [JOB_ID]

Arguments:
  [JOB_ID]  Optional job ID. If omitted, shows queue overview.
```

**Queue overview (no args)**:
```
$ claw status

Queue Status
───────────────────────────────
Pending:    5  (P9: 1, P5: 3, P0: 1)
Running:    2
Completed:  47  (today)
Failed:     3   (today)

Workers: 2 active / 4 capacity
Cost today: $12.34

Recent:
  f47ac10b  running   "Review the code in..."     worker-1-task-0  2m ago
  a1b2c3d4  completed "Refactor database..."      worker-1-task-1  5m ago  $0.42
  e5f6a7b8  pending   "Analyze test coverage..."                    8m ago
```

**Job detail (with ID)**:
```
$ claw status f47ac10b

Job: f47ac10b-58cc-4372-a567-0e02b2c3d479
Status:     running
Source:     cli
Priority:   8
Tags:       pr-review, security

Prompt:     Review this PR for security issues
Skills:     code-review, security-audit
Model:      sonnet
Budget:     $2.00

Working dir: /repos/my-project
Output:      webhook → https://hooks.slack.com/xyz

Created:    2026-03-15 22:30:00
Started:    2026-03-15 22:30:02
Worker:     worker-1-task-0
Retries:    0
```

### 3.3 `claw result` — Get Job Result

```
claw result <JOB_ID>

Arguments:
  <JOB_ID>  The job ID

Options:
  --format <FMT>  Output format: text (default), json, raw
```

**Example**:
```
$ claw result f47ac10b

## Security Review of PR #42

### Critical Issues
1. **SQL Injection** (line 42): User input passed directly to query...

### Minor Issues
1. Missing input validation on the `email` field...

### Verdict: Request Changes

Cost: $0.42 | Duration: 2m 13s | Model: sonnet
```

### 3.4 `claw logs` — View Job Logs

```
claw logs <JOB_ID>

Arguments:
  <JOB_ID>  The job ID

Options:
  -f, --follow       Stream live logs (for running jobs)
  -n, --lines <N>    Show last N lines [default: all]
      --raw          Show raw JSON lines instead of formatted
```

**Example (static)**:
```
$ claw logs f47ac10b

[22:30:02] assistant: I'll start by reading the main source file.
[22:30:03] tool: Read → src/main.rs
[22:30:04] tool_result: (142 lines read)
[22:30:07] assistant: I found several security concerns...
[22:30:08] tool: Grep → "sql" in src/
[22:30:09] tool_result: 3 matches
[22:32:15] result: ## Security Review...
```

**Example (--follow for running job)**:
```
$ claw logs f47ac10b --follow

[22:30:02] assistant: I'll start by reading...
[22:30:03] tool: Read → src/main.rs
... (new lines appear in real-time) ...
^C
```

### 3.5 `claw list` — List Jobs

```
claw list [OPTIONS]

Options:
  --status <STATUS>   Filter: pending, running, completed, failed, cancelled
  --source <SOURCE>   Filter: cli, api, cron, filewatcher
  --tag <TAG>         Filter by tag (repeatable)
  --limit <N>         Max results [default: 20]
  --sort <SORT>       Sort: newest (default), oldest, priority
  --search <TEXT>     Substring search in prompt
```

**Example**:
```
$ claw list --status completed --tag pr-review --limit 5

ID         Status     Prompt                          Cost    Duration  Created
─────────  ─────────  ─────────────────────────────   ──────  ────────  ──────────
f47ac10b   completed  Review this PR for security...  $0.42   2m 13s   15m ago
a1b2c3d4   completed  Review PR #41 changes           $0.38   1m 55s   2h ago
b3c4d5e6   completed  Review PR #40 refactoring       $0.51   3m 02s   5h ago
c4d5e6f7   completed  Review PR #39 new feature       $0.67   4m 11s   1d ago
d5e6f7a8   completed  Review PR #38 bugfix            $0.29   1m 22s   1d ago

Showing 5 of 23 total
```

### 3.6 `claw cancel` — Cancel a Job

```
claw cancel <JOB_ID>
```

**Example**:
```
$ claw cancel f47ac10b
Job f47ac10b cancelled.

$ claw cancel f47ac10b
Error: Job f47ac10b is already completed (cannot cancel).
```

### 3.7 `claw delete` — Delete a Job

```
claw delete <JOB_ID>

Options:
  --force   Delete even if job is pending/running (cancels first)
```

### 3.8 `claw skill` — Skill Management

```
claw skill <SUBCOMMAND>

Subcommands:
  create   Create a new skill
  list     List all skills
  show     Show skill details and content
  edit     Update a skill's content
  delete   Delete a skill
  sync     Re-sync skills from filesystem to Redis
```

**Create**:
```bash
# From file
claw skill create --id security-audit --name "Security Audit" \
    --type template --file ./security-template.md \
    --tags security,review \
    --description "OWASP-focused security review"

# Inline
claw skill create --id concise --name "Concise Output" \
    --type template --content "Be extremely concise. Max 3 sentences." \
    --tags format
```

**List**:
```
$ claw skill list

ID                Type           Tags                Description
────────────────  ────────────   ──────────────────  ────────────────────────────
code-review       template       review, quality     Structured code review criteria
security-audit    template       security, review    OWASP-focused security review
rust-project      claude_config  rust, config        Rust project conventions
run-tests         script         testing             Generic test runner
```

**Show**:
```
$ claw skill show code-review

ID:          code-review
Name:        Code Review
Type:        template
Tags:        review, quality
Description: Structured code review criteria
Created:     2026-03-01 10:00:00
Updated:     2026-03-10 15:30:00

Content:
───────────
When reviewing code, evaluate the following dimensions:

1. **Correctness**: Does the code do what it claims? Edge cases?
2. **Security**: Any OWASP Top 10 vulnerabilities?
...
```

**Edit**:
```bash
claw skill edit code-review --file ./updated-template.md
# or
claw skill edit code-review --content "New content here"
```

### 3.9 `claw cron` — Cron Schedule Management

```
claw cron <SUBCOMMAND>

Subcommands:
  create    Create a new cron schedule
  list      List all cron schedules
  show      Show cron details
  edit      Update a cron schedule
  enable    Enable a cron schedule
  disable   Disable a cron schedule
  delete    Delete a cron schedule
  trigger   Manually trigger a cron job now
```

**Create**:
```bash
claw cron create \
    --name "Morning PR Review" \
    --schedule "0 9 * * MON-FRI" \
    --prompt "Review all open PRs and post summaries" \
    --skill code-review \
    --working-dir /repos/main-project \
    --output webhook:https://hooks.slack.com/... \
    --priority 6
```

**List**:
```
$ claw cron list

ID         Name               Schedule              Enabled  Last Run          Next Run
─────────  ─────────────────  ────────────────────  ───────  ────────────────  ────────────────
a1b2c3d4   Morning PR Review  0 9 * * MON-FRI      yes      2026-03-14 09:00  2026-03-16 09:00
b2c3d4e5   Nightly Tests      0 2 * * *             yes      2026-03-15 02:00  2026-03-16 02:00
c3d4e5f6   Weekly Report      0 17 * * FRI          no       2026-03-07 17:00  —
```

**Trigger now**:
```
$ claw cron trigger a1b2c3d4
Triggered cron "Morning PR Review"
Job submitted: f47ac10b-58cc-4372-a567-0e02b2c3d479
```

### 3.10 `claw workers` — Worker Status

```
$ claw workers

Worker ID           Status  Current Job   Job Prompt                    Uptime
──────────────────  ──────  ───────────   ────────────────────────────  ──────
worker-1-task-0     busy    f47ac10b      "Review this PR for..."       2h 15m
worker-1-task-1     idle    —             —                             2h 15m
worker-2-task-0     busy    a1b2c3d4      "Refactor database module..." 1h 30m
worker-2-task-1     idle    —             —                             1h 30m
```

## 4. Configuration File

### 4.1 File Locations (in precedence order)

1. `--config <path>` flag
2. `CLAW_CONFIG` environment variable
3. `./claw.toml` (project-local)
4. `~/.claw/config.toml` (user-global)

### 4.2 Format

```toml
# ~/.claw/config.toml

# Redis connection
redis_url = "redis://127.0.0.1:6379"

# Default values for job submission
[defaults]
model = "sonnet"
max_budget_usd = 1.00
output = "redis"                    # "redis", "file:/path", "webhook:url"
working_dir = "."
priority = 5
timeout_secs = 1800

# Skills
[skills]
dir = "./skills"                    # Directory for filesystem skills

# Output
[output]
dir = "./output"                    # Default file output directory

# Display
[display]
color = true                        # Colored terminal output
timestamps = "relative"             # "relative" (5m ago) or "absolute" (2026-03-15 22:30:00)
```

### 4.3 Environment Variable Overrides

Every config option can be overridden by an environment variable:

| Config Key | Environment Variable |
|-----------|---------------------|
| `redis_url` | `CLAW_REDIS_URL` |
| `defaults.model` | `CLAW_DEFAULT_MODEL` |
| `defaults.max_budget_usd` | `CLAW_DEFAULT_BUDGET` |
| `defaults.working_dir` | `CLAW_DEFAULT_WORKING_DIR` |
| `defaults.priority` | `CLAW_DEFAULT_PRIORITY` |
| `defaults.timeout_secs` | `CLAW_DEFAULT_TIMEOUT` |
| `skills.dir` | `CLAW_SKILLS_DIR` |
| `output.dir` | `CLAW_OUTPUT_DIR` |

## 5. Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | General error (invalid args, Redis connection failure) |
| 2 | Job not found |
| 3 | Job in wrong state for operation (e.g., cancel on completed job) |
| 4 | Timeout (--wait timed out) |

## 6. Clap Implementation Sketch

```rust
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "claw", about = "Claw Machine — Job queue for Claude Code")]
pub struct Cli {
    #[arg(long, env = "CLAW_REDIS_URL", default_value = "redis://127.0.0.1:6379")]
    pub redis_url: String,

    #[arg(long)]
    pub config: Option<PathBuf>,

    #[arg(long, global = true)]
    pub json: bool,

    #[arg(long, short, global = true)]
    pub quiet: bool,

    #[arg(long, short, global = true)]
    pub verbose: bool,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Submit a new job
    Submit(SubmitArgs),
    /// Show queue status or job detail
    Status(StatusArgs),
    /// Get job result
    Result(ResultArgs),
    /// View job logs
    Logs(LogsArgs),
    /// List jobs
    List(ListArgs),
    /// Cancel a job
    Cancel(CancelArgs),
    /// Delete a job
    Delete(DeleteArgs),
    /// Manage skills
    Skill {
        #[command(subcommand)]
        command: SkillCommands,
    },
    /// Manage cron schedules
    Cron {
        #[command(subcommand)]
        command: CronCommands,
    },
    /// Show worker status
    Workers,
}

#[derive(clap::Args)]
pub struct SubmitArgs {
    /// The task prompt (or "-" for stdin)
    pub prompt: String,

    #[arg(short, long = "skill", action = clap::ArgAction::Append)]
    pub skills: Vec<String>,

    #[arg(long = "skill-tag", action = clap::ArgAction::Append)]
    pub skill_tags: Vec<String>,

    #[arg(short = 'd', long)]
    pub working_dir: Option<PathBuf>,

    #[arg(short, long)]
    pub model: Option<String>,

    #[arg(short, long)]
    pub budget: Option<f64>,

    #[arg(long, action = clap::ArgAction::Append)]
    pub allowed_tools: Vec<String>,

    #[arg(short, long)]
    pub output: Option<String>,

    #[arg(short, long, default_value = "5")]
    pub priority: u8,

    #[arg(short, long, action = clap::ArgAction::Append)]
    pub tag: Vec<String>,

    #[arg(long)]
    pub timeout: Option<u64>,

    #[arg(short, long)]
    pub wait: bool,

    #[arg(short, long)]
    pub follow: bool,
}
```
