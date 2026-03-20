# Skill Package Format Specification

Version: 1.0

## Overview

A skill package is a ZIP file containing a SKILL.md file, an optional manifest, and optional bundled files. Skills are Claude Code instructions deployed to `.claude/skills/{id}/` in workspaces during job execution.

## ZIP Structure

```
my-skill.zip
├── SKILL.md           # Required: skill content (instructions for Claude)
├── manifest.json      # Recommended: package metadata
├── scripts/           # Optional: bundled files
│   └── run-tests.sh
└── references/        # Optional: reference files
    └── style-guide.md
```

### Root Directory Stripping

If all files share a common root directory prefix (e.g., `my-skill/SKILL.md`), it is automatically stripped on import. This allows exporting a directory as a ZIP without restructuring.

## SKILL.md

The primary content file. Contains instructions that Claude Code follows during job execution.

### Format

Plain markdown. Optionally includes YAML frontmatter:

```markdown
---
name: Code Review
description: Reviews code for bugs, style issues, and best practices
---

# Code Review Skill

When reviewing code, follow these steps:
1. Check for bugs and logic errors
2. Verify error handling
3. Review naming conventions
...
```

Frontmatter fields (`name`, `description`) are used as fallbacks if not provided in the manifest or upload form.

## manifest.json

Package metadata for import/export and marketplace listing.

```json
{
  "format": "claw-skill-v1",
  "id": "code-review",
  "name": "Code Review",
  "version": "1.2.0",
  "author": "rob",
  "license": "MIT",
  "description": "Reviews code for bugs and style issues",
  "tags": ["review", "quality", "linting"]
}
```

### Fields

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `format` | string | Yes | Must be `"claw-skill-v1"` |
| `id` | string | Yes | Unique identifier (e.g., `code-review`, `python-linter`) |
| `name` | string | Yes | Display name |
| `version` | string | No | Semantic version (e.g., `1.0.0`) |
| `author` | string | No | Package author |
| `license` | string | No | License identifier (e.g., `MIT`, `Apache-2.0`) |
| `description` | string | No | Short description |
| `tags` | string[] | No | Categorization tags |

### Import Behavior

When importing a ZIP with `manifest.json`:
- Manifest values auto-populate the import form
- Multipart form fields override manifest values (user can change ID on import)
- If no manifest, all metadata must be provided in the upload form

## Bundled Files

Any files besides `SKILL.md` and `manifest.json` are stored as bundled files. They are deployed alongside the skill in the workspace.

- File paths are relative (e.g., `scripts/run-tests.sh`)
- Files in `scripts/` subdirectory are automatically made executable (`chmod +x`)
- Only text files are supported (binary files are skipped)
- Maximum file size: 10MB per file
- Maximum total size: 50MB
- Maximum entry count: 5000

## Deployment

When a job uses a skill, the worker deploys it to:

```
{workspace}/.claude/skills/{skill-id}/
├── SKILL.md
├── scripts/
│   └── run-tests.sh
└── references/
    └── style-guide.md
```

Claude Code discovers skills in `.claude/skills/` natively.

## API Endpoints

```
POST   /api/v1/skills/upload        — import skill from ZIP (multipart: file, id, name, description, tags)
GET    /api/v1/skills/{id}/download — export skill as ZIP
```

## Examples

### Minimal Skill ZIP

```
minimal-skill.zip
└── SKILL.md
```

SKILL.md:
```markdown
Always write tests for new code. Use the project's existing test framework.
```

### Full Skill ZIP

```
full-skill.zip
├── SKILL.md
├── manifest.json
├── scripts/
│   ├── lint.sh
│   └── test.sh
└── templates/
    └── pr-template.md
```
