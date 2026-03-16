---
name: create-skill
description: Creates new Agent Skills following the agentskills.io specification. Use when asked to create a skill, build a new capability, package instructions as a skill, or when the user says "create a skill" or "make a skill for".
license: MIT
metadata:
  author: claudecodeclaw
  version: "1.0"
  spec-url: https://agentskills.io/specification
---

# Create Agent Skill

You are a skill author. When asked to create a new skill, follow this process exactly.

## Step 1: Understand the Request

Ask clarifying questions if needed:
- What should the skill do?
- What tools/scripts does it need?
- What reference material should be included?
- Who is the target audience?

## Step 2: Choose a Name

The skill name MUST:
- Be 1-64 characters
- Use only lowercase letters, numbers, and hyphens
- Not start or end with a hyphen
- Not contain consecutive hyphens
- Match the parent directory name

Good: `code-review`, `pdf-processing`, `data-analysis`
Bad: `Code-Review`, `-pdf`, `pdf--processing`

## Step 3: Write the SKILL.md

Create the directory and SKILL.md file:

```
{skill-name}/
├── SKILL.md          # Required
├── scripts/          # Optional: executable code
├── references/       # Optional: documentation
└── assets/           # Optional: templates, resources
```

### SKILL.md Structure

```markdown
---
name: {skill-name}
description: {1-1024 chars}. Describe what it does AND when to use it. Include keywords that help agents identify relevant tasks.
license: MIT
metadata:
  author: {author}
  version: "1.0"
---

# {Skill Title}

## When to use this skill
{Describe the trigger conditions — what tasks or user requests should activate this skill}

## Instructions
{Step-by-step instructions for the agent to follow}

## Examples
{Show concrete input → output examples}

## Edge Cases
{Handle common failure modes and edge cases}
```

### Required Frontmatter

| Field | Required | Rules |
|-------|----------|-------|
| `name` | Yes | Lowercase, hyphens, 1-64 chars, matches directory name |
| `description` | Yes | 1-1024 chars, describes WHAT it does and WHEN to use it |
| `license` | No | License name or reference to bundled LICENSE file |
| `compatibility` | No | Environment requirements (tools, network, etc.) |
| `metadata` | No | Arbitrary key-value pairs |
| `allowed-tools` | No | Space-delimited pre-approved tools (experimental) |

### Description Best Practices

GOOD: "Extracts text and tables from PDF files, fills PDF forms, and merges multiple PDFs. Use when working with PDF documents or when the user mentions PDFs, forms, or document extraction."

BAD: "Helps with PDFs."

The description is used for skill discovery — agents read ALL descriptions at startup to decide which skills are relevant. Make it specific and keyword-rich.

## Step 4: Add Supporting Files

### scripts/ (optional)
Executable code the agent can run:
- Make scripts self-contained
- Include error handling
- Add helpful error messages
- Use `#!/usr/bin/env bash` or `#!/usr/bin/env python3` shebangs
- Make executable: `chmod +x scripts/*.sh`

### references/ (optional)
Additional documentation loaded on demand:
- Keep files focused (one topic per file)
- Name descriptively: `api-reference.md`, `troubleshooting.md`
- Agents load these only when needed, so smaller = better

### assets/ (optional)
Static resources:
- Templates (document templates, config templates)
- Data files (schemas, lookup tables)
- Images (diagrams, examples)

## Step 5: Validate the Skill

After creating, verify:
1. `SKILL.md` has valid YAML frontmatter with `name` and `description`
2. `name` field matches the directory name
3. `name` follows naming rules (lowercase, hyphens, no consecutive hyphens)
4. `description` is 1-1024 characters and describes WHAT + WHEN
5. Body content is < 500 lines (move details to references/)
6. File references use relative paths from skill root
7. Scripts have proper shebangs and are executable

## Step 6: Package as ZIP

For distribution via ClaudeCodeClaw:
1. Create a ZIP of the skill directory
2. Upload via Skills → Import ZIP in the admin console
3. The ZIP root should be the skill directory (e.g., `code-review/SKILL.md`)

## Progressive Disclosure Rules

Skills use three levels of context:
1. **Metadata** (~100 tokens): `name` + `description` — loaded at startup for ALL skills
2. **Instructions** (< 5000 tokens): Full SKILL.md body — loaded when skill activates
3. **Resources** (as needed): scripts/, references/, assets/ — loaded only when required

Keep SKILL.md under 500 lines. Move detailed material to references/.

## Output Format

When creating a skill, output:
1. The complete directory structure
2. The full SKILL.md file content
3. Any scripts with proper shebangs
4. Any reference files
5. Instructions for packaging as a ZIP
