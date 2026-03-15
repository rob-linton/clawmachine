# Gap Analysis: Plan vs Implementation

Perform a comprehensive gap analysis comparing the current codebase implementation against a plan in `~/.claude/plans/`.

**Plan name argument:** $ARGUMENTS

## Instructions

Use maximum available thinking tokens. Be thorough and methodical.

## 1. Find the Plan

**If a plan name was specified above:**
1. Look for a file matching that name in `~/.claude/plans/` (with or without .md extension)
2. If not found, list available plans and report the error

**If no plan name was specified (empty argument):**
1. List all `.md` files in `~/.claude/plans/` directory sorted by modification time (newest first)
2. If no plans exist, report this and exit
3. Take the 3 most recent plans
4. For each plan, read the first line that starts with `#` to get the plan title
5. Use the `AskUserQuestion` tool to present a selection picker with these options:
   - Each option label: the plan's title (from the `#` heading)
   - Each option description: the filename
   - Let the user select which plan to analyze
6. Use the selected plan file for analysis

## 2. Parse the Plan

Read the selected plan file and extract:
- **Goals/Objectives**: What the plan intended to achieve
- **Tasks/Steps**: Individual implementation items
- **Architecture decisions**: Patterns, structures, or approaches specified
- **Files to modify/create**: Any files explicitly mentioned
- **Redis keys**: Any new keys mentioned
- **API endpoints**: Any new endpoints mentioned
- **Configuration changes**: Any config additions

## 3. Analyze Implementation Status

For each task/step in the plan:

### Status Categories
- **COMPLETE**: Fully implemented as specified
- **PARTIAL**: Started but incomplete or diverged from plan
- **NOT STARTED**: No evidence of implementation
- **MODIFIED**: Implemented differently than planned (note why)

### Evidence Gathering
- Search for relevant code changes
- Check if mentioned files exist and contain expected code
- Verify Redis keys are documented and implemented
- Verify API endpoints exist and function as planned
- Check that tests exist if mentioned in plan

## 4. Identify Gaps

### Missing Implementations
- List tasks that have no corresponding code
- Note any TODO/FIXME comments referencing planned work

### Incomplete Implementations
- Identify partial implementations
- Note what's missing from each

### Deviations
- Document where implementation differs from plan
- Assess if deviation is acceptable or needs reconciliation

### Orphaned Code
- Identify implemented features not in the plan
- Assess if this is scope creep or legitimate addition

## 5. Documentation Verification

Check if the following are in sync with implementation:
- CLAUDE.md reflects all implemented features
- API.md documents new endpoints
- REDIS.md documents new keys
- Any other relevant documentation

## Output Format

```
## Gap Analysis Report

**Plan File**: [filename]
**Analysis Date**: [date]

### Implementation Summary

| Status | Count |
|--------|-------|
| Complete | X |
| Partial | X |
| Not Started | X |
| Modified | X |

### Detailed Findings

#### COMPLETE Tasks
- [x] Task description - Location: `file:line`

#### PARTIAL Tasks
- [ ] Task description
  - Done: [what's implemented]
  - Missing: [what's not]
  - Location: `file:line`

#### NOT STARTED Tasks
- [ ] Task description
  - Expected: [what was planned]
  - Notes: [any blockers or dependencies]

#### MODIFIED Tasks
- [~] Task description
  - Planned: [original spec]
  - Actual: [what was done]
  - Reason: [why it changed, if known]

### Documentation Gaps
- [ ] CLAUDE.md: [missing items]
- [ ] API.md: [missing items]
- [ ] REDIS.md: [missing items]

### Recommendations

1. Priority items to complete
2. Documentation to update
3. Deviations to reconcile or accept

### Next Steps
[Actionable items to close gaps]
```

## Notes

- If no `~/.claude/plans/` directory exists, suggest creating one
- If a plan name is provided, use that specific plan
- If no plan name is provided, use the most recent plan file by modification time
- Focus on factual analysis, not judgment of plan quality
