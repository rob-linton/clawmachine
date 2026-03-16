# Agent Skills Specification Summary

Source: https://agentskills.io/specification

## Directory Structure

```
skill-name/
├── SKILL.md          # Required: metadata + instructions
├── scripts/          # Optional: executable code
├── references/       # Optional: documentation
├── assets/           # Optional: templates, resources
└── ...               # Any additional files or directories
```

## SKILL.md Frontmatter Fields

| Field | Required | Constraints |
|-------|----------|-------------|
| `name` | Yes | 1-64 chars. Lowercase letters, numbers, hyphens only. No leading/trailing/consecutive hyphens. Must match directory name. |
| `description` | Yes | 1-1024 chars. Non-empty. Describes what + when. |
| `license` | No | License name or reference to bundled file. |
| `compatibility` | No | 1-500 chars. Environment requirements. |
| `metadata` | No | Arbitrary string key-value mapping. |
| `allowed-tools` | No | Space-delimited tool list (experimental). |

## Name Validation Rules

- Only lowercase alphanumeric + hyphens
- No uppercase
- No leading hyphen
- No trailing hyphen
- No consecutive hyphens (--)
- Must match parent directory name

## Progressive Disclosure Levels

1. **Metadata** (~100 tokens): name + description loaded at startup for all skills
2. **Instructions** (< 5000 tokens recommended): SKILL.md body loaded on activation
3. **Resources** (as needed): scripts/references/assets loaded on demand

## Recommended SKILL.md Size

- Under 500 lines
- Move detailed reference material to separate files in references/
- Keep file references one level deep from SKILL.md
