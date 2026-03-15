# Skills System — Reusable Knowledge for Jobs

## 1. Concept

Skills are the mechanism for shared knowledge between jobs. Instead of persistent conversation history (which would accumulate context window usage), skills provide discrete, versioned, reusable units of expertise that get injected into each job's prompt or execution environment.

A skill answers the question: **"What should Claude know or have access to for this type of task?"**

## 2. Skill Taxonomy

### 2.1 Prompt Templates (`type: template`)

**What**: Markdown-formatted instructions, guidelines, or context that get prepended to the job's prompt.

**When to use**: When you want to standardize how Claude approaches a category of task — review criteria, output format requirements, domain-specific knowledge, or behavioral constraints.

**How injected**: Wrapped in `<skill name="...">` XML tags and prepended to the prompt. Multiple templates are concatenated in order.

**Examples**:

```markdown
# code-review template
When reviewing code, evaluate the following dimensions:

1. **Correctness**: Does the code do what it claims? Edge cases?
2. **Security**: Any OWASP Top 10 vulnerabilities? Input validation?
3. **Performance**: O(n) analysis, unnecessary allocations, blocking I/O?
4. **Readability**: Clear naming, appropriate comments, consistent style?
5. **Testing**: Are there tests? Do they cover the changes?

Format your review as:
- Summary (1-2 sentences)
- Issues (severity: critical/major/minor)
- Suggestions (optional improvements)
- Verdict (approve / request changes)
```

```markdown
# json-output template
IMPORTANT: Your final output MUST be valid JSON matching this schema:
{
  "summary": "string",
  "findings": [{"title": "string", "severity": "string", "details": "string"}],
  "recommendation": "string"
}
Do not include any text outside the JSON block.
```

### 2.2 CLAUDE.md Configs (`type: claude_config`)

**What**: Content that gets written to `CLAUDE.md` in the job's working directory before execution. Claude Code natively reads this file for project-level instructions.

**When to use**: When you want to set project-level conventions, tool preferences, or behavioral rules that Claude Code's built-in CLAUDE.md system understands.

**How injected**: Written to `{working_dir}/CLAUDE.md` (appended if one already exists, with a `## ClaudeCodeClaw Skills` section separator).

**Examples**:

```markdown
# rust-project config
# Rust Project Conventions

- Use `thiserror` for error types, not manual `impl Display`
- Prefer `&str` over `String` in function parameters
- Use `tracing` for logging, not `println!` or `log`
- Error handling: use `?` propagation, avoid `.unwrap()` in library code
- Tests go in the same file as the code they test (`#[cfg(test)]` module)
- Run `cargo clippy -- -D warnings` before considering code complete
```

```markdown
# typescript-project config
# TypeScript Project Conventions

- Use strict TypeScript (`strict: true` in tsconfig)
- Prefer `interface` over `type` for object shapes
- Use `zod` for runtime validation at API boundaries
- Error handling: use Result types (neverthrow), not try/catch for control flow
- Tests: vitest, co-located `.test.ts` files
```

### 2.3 Scripts (`type: script`)

**What**: Executable shell scripts or programs that Claude Code can invoke during job execution.

**When to use**: When Claude needs access to tools or workflows that aren't built into Claude Code — custom linters, deployment scripts, database queries, data fetchers, etc.

**How injected**: Written to `{working_dir}/.claw/scripts/{skill_id}`, made executable (`chmod +x`). The prompt includes a notice that these scripts are available.

**Examples**:

```bash
# run-tests script
#!/bin/bash
set -euo pipefail
# Run the full test suite and output results
cargo test --workspace --no-fail-fast 2>&1
echo "---"
echo "Exit code: $?"
```

```bash
# fetch-pr-data script
#!/bin/bash
set -euo pipefail
# Fetch PR data from GitHub API
# Usage: ./fetch-pr-data.sh <owner> <repo> <pr_number>
gh api repos/$1/$2/pulls/$3 --jq '{
  title: .title,
  body: .body,
  diff_url: .diff_url,
  changed_files: .changed_files,
  additions: .additions,
  deletions: .deletions,
  author: .user.login
}'
```

```python
# analyze-logs script
#!/usr/bin/env python3
"""Analyze application logs and produce summary statistics."""
import sys
import json
from collections import Counter

log_file = sys.argv[1] if len(sys.argv) > 1 else "/var/log/app.log"
# ... analysis logic ...
print(json.dumps(summary, indent=2))
```

## 3. Skill Storage

### 3.1 Primary Storage: Redis

Skills are stored as Redis hashes at `claw:skill:{id}`:

```
claw:skill:code-review
    id          = "code-review"
    name        = "Code Review"
    skill_type  = "template"
    content     = "When reviewing code, evaluate..."
    description = "Structured code review guidelines"
    tags        = '["review","quality"]'
    created_at  = "2026-03-01T10:00:00Z"
    updated_at  = "2026-03-01T10:00:00Z"

claw:skills:index = {"code-review", "rust-project", "run-tests", ...}
```

### 3.2 Filesystem Seeding

On startup, the worker (and optionally the API server) scans the `skills/` directory and loads any skills not already in Redis:

```
skills/
├── templates/
│   ├── code-review.md          → id: "code-review", type: template
│   ├── security-audit.md       → id: "security-audit", type: template
│   └── json-output.md          → id: "json-output", type: template
├── claude-configs/
│   ├── rust-project.md         → id: "rust-project", type: claude_config
│   └── typescript-project.md   → id: "typescript-project", type: claude_config
└── scripts/
    ├── run-tests.sh            → id: "run-tests", type: script
    └── fetch-pr-data.sh        → id: "fetch-pr-data", type: script
```

**File-to-skill mapping**:
- Filename (without extension) → `id`
- First `# ` heading in file → `name` (or filename if no heading)
- Directory → `skill_type` (`templates/` → template, etc.)
- Content → `content`
- Optional YAML frontmatter for `description` and `tags`:

```markdown
---
description: Security-focused code review for web applications
tags: [security, review, web]
---
# Security Audit

When auditing code for security vulnerabilities...
```

### 3.3 Precedence

If a skill exists in both Redis and the filesystem:
- **Redis wins** — filesystem seeds are only loaded if the skill doesn't already exist in Redis
- To force-update from filesystem, delete the skill from Redis first (or use `claw skill sync` command)

## 4. Skill Composition

### 4.1 Direct Reference (`skill_ids`)

Jobs explicitly list skill IDs to include:

```json
{
    "prompt": "Review this PR",
    "skill_ids": ["code-review", "rust-project"]
}
```

Order matters — skills are injected in the order listed.

### 4.2 Tag Matching (`skill_tags`)

Jobs specify tags, and all skills matching any tag are included:

```json
{
    "prompt": "Review this Rust code",
    "skill_tags": ["rust", "review"]
}
```

This includes any skill that has at least one matching tag. Useful for broad capability matching without knowing specific skill IDs.

### 4.3 Combined

Both can be used together. Explicit `skill_ids` are injected first, then tag-matched skills (deduped):

```json
{
    "prompt": "Review this PR for security issues",
    "skill_ids": ["security-audit"],
    "skill_tags": ["rust"]
}
```

This guarantees `security-audit` is first, then any `rust`-tagged skills follow.

## 5. Skill Lifecycle

### 5.1 Creation

Via CLI:
```bash
# From a file
claw skill create --id code-review --name "Code Review" --type template \
    --file ./my-review-template.md --tags review,quality

# Inline content
claw skill create --id quick-check --name "Quick Check" --type template \
    --content "Check this code for obvious bugs and return a one-line summary." \
    --tags quick

# From stdin
cat template.md | claw skill create --id my-skill --name "My Skill" --type template --stdin
```

Via API:
```http
POST /api/v1/skills
{"id": "code-review", "name": "Code Review", "skill_type": "template", ...}
```

Via UI:
The Skill Editor screen provides a code editor with syntax highlighting.

### 5.2 Versioning

Skills are **mutable** — updating a skill changes it for all future jobs. There is no built-in version history.

If version tracking is needed:
- Use git to version the `skills/` directory
- Include version in the skill ID: `code-review-v2`
- Store update history in the skill's description

### 5.3 Deletion

Deleting a skill removes it from Redis. Jobs that reference the deleted skill by ID will log a warning but continue executing (the skill is simply not injected).

## 6. Skill Injection Detail

### 6.1 Template Injection

Templates are wrapped in XML tags to clearly delineate them from the user's prompt:

```xml
<skill name="code-review">
When reviewing code, evaluate the following dimensions:

1. **Correctness**: Does the code do what it claims?
...
</skill>

<skill name="rust-conventions">
Rust-specific conventions to follow:
- Use thiserror for error types
...
</skill>

Review the changes in the latest PR and provide feedback.
```

The `<skill>` tags help Claude understand that this is injected context, not the user's direct request.

### 6.2 CLAUDE.md Injection

If the working directory already has a `CLAUDE.md`, the injected config is appended:

```markdown
[existing CLAUDE.md content]

---

## ClaudeCodeClaw Injected Skills

[claude_config skill content]
```

After the job completes, the worker restores the original `CLAUDE.md` (or removes the injected section). This cleanup ensures the working directory isn't polluted by repeated job runs.

### 6.3 Script Injection

Scripts are written to a job-specific subdirectory to avoid conflicts between parallel jobs:

```
{working_dir}/
└── .claw/
    └── scripts/
        └── {job_id}/
            ├── run-tests.sh      (chmod +x)
            ├── fetch-pr-data.sh  (chmod +x)
            └── analyze-logs.py   (chmod +x)
```

The prompt includes:
```
You have access to the following executable scripts in .claw/scripts/{job_id}/:
- run-tests.sh: Run the full test suite and output results
- fetch-pr-data.sh: Fetch PR data from GitHub API (usage: ./fetch-pr-data.sh <owner> <repo> <pr>)
- analyze-logs.py: Analyze application logs and produce summary statistics

Run them with their full path if needed for your task.
```

After the job completes, the worker removes the `.claw/scripts/{job_id}/` directory.

## 7. Built-in Skills Library

The project ships with a curated set of skills in the `skills/` directory:

### Templates
| ID | Description |
|----|-------------|
| `code-review` | General code review criteria |
| `security-audit` | Security-focused review (OWASP, injection, auth) |
| `refactor` | Code refactoring guidelines (DRY, SOLID, complexity) |
| `json-output` | Force JSON output format |
| `concise` | Request concise, minimal output |
| `explain` | Request detailed explanations |

### CLAUDE.md Configs
| ID | Description |
|----|-------------|
| `rust-project` | Rust conventions (error handling, naming, tooling) |
| `typescript-project` | TypeScript conventions (strict, zod, vitest) |
| `python-project` | Python conventions (typing, pytest, ruff) |

### Scripts
| ID | Description |
|----|-------------|
| `run-tests` | Generic test runner (detects language/framework) |
| `git-diff` | Get the diff for a branch or commit range |
| `lint-check` | Run linters and format checkers |

## 8. Skill Design Best Practices

### 8.1 Template Skills

- **Be specific**: Instead of "review the code", provide concrete evaluation criteria
- **Include output format**: Tell Claude exactly how to structure the response
- **Keep focused**: One skill per concern (don't combine review + refactoring + testing in one template)
- **Use markdown**: Claude interprets markdown formatting naturally

### 8.2 CLAUDE.md Config Skills

- **Project-scoped conventions**: Things that should apply to every interaction with a project
- **Don't duplicate templates**: Use templates for task-specific instructions, configs for project-wide rules
- **Keep concise**: CLAUDE.md is read on every invocation — keep it focused

### 8.3 Script Skills

- **Use `set -euo pipefail`**: Fail fast on errors
- **Output to stdout**: Claude reads stdout for results
- **Accept arguments**: Use positional args or env vars for input
- **Exit codes matter**: Non-zero exit = Claude knows the script failed
- **No interactive input**: Scripts must run non-interactively
