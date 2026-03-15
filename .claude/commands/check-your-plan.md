# Check Plan: Identify Weaknesses and Blind Spots

Perform a critical review of a plan in `~/.claude/plans/` to identify edge cases not considered, misunderstandings, omissions, and areas where the plan does not fully address the problem.

**Plan name argument:** $ARGUMENTS

## Instructions

Use maximum available thinking tokens. Be thorough, skeptical, and methodical. Your role is to be a critical reviewer, not a cheerleader.

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
   - Let the user select which plan to critique
6. Use the selected plan file for analysis

## 2. Understand the Plan Context

Read the selected plan file and extract:
- **Original Problem/Issue**: What problem is being solved?
- **Goals/Objectives**: What the plan intends to achieve
- **Proposed Solution**: The implementation approach
- **Files to modify/create**: Any files explicitly mentioned
- **Assumptions**: Both explicit and implicit assumptions made
- **Dependencies**: External systems, services, or code relied upon

## 3. Critical Analysis Categories

### 3.1 Edge Cases Not Considered

Search for scenarios the plan may not handle:

- **Input Edge Cases**
  - Empty/null/undefined values
  - Extremely large inputs (max int, huge strings, massive files)
  - Malformed or invalid data formats
  - Unicode/special characters
  - Boundary values (0, -1, MAX_VALUE)

- **State Edge Cases**
  - Race conditions and concurrency issues
  - Partial failures (some operations succeed, others fail)
  - Interrupted operations (network drops mid-request)
  - Stale data / cache invalidation scenarios
  - Session expiry during long operations

- **User Behavior Edge Cases**
  - Rapid repeated submissions (double-click)
  - Back button / navigation during operations
  - Multiple tabs/windows with same session
  - Browser refresh during async operations
  - Copy/paste of unexpected content

- **System Edge Cases**
  - Service unavailability (database, storage, external APIs)
  - Timeout scenarios
  - Disk full / storage quota exceeded
  - Memory pressure
  - Clock skew / timezone issues

### 3.2 Potential Misunderstandings

Look for signs that the plan may have misunderstood:

- **Requirements Misinterpretation**
  - Does the solution actually solve the stated problem?
  - Are there implicit requirements not addressed?
  - Does the plan conflate symptoms with root causes?

- **Codebase Misunderstanding**
  - Does the plan assume code works differently than it does?
  - Are there existing utilities/patterns being ignored?
  - Does it duplicate functionality that already exists?
  - Does it modify the wrong file or function?

- **Domain Misunderstanding**
  - Are business rules correctly understood?
  - Are security requirements properly interpreted?
  - Are compliance/regulatory constraints considered?

- **Technical Misunderstanding**
  - Are API contracts correctly understood?
  - Are database schema assumptions correct?
  - Are threading/async behaviors correctly modeled?

### 3.3 Omissions of Important Issues

Identify what the plan fails to address:

- **Security Omissions**
  - Authentication/authorization checks missing?
  - Input validation/sanitization overlooked?
  - Sensitive data exposure risks?
  - Audit logging requirements?

- **Error Handling Omissions**
  - What happens when X fails?
  - Are all error paths defined?
  - Are error messages user-friendly and secure (no info disclosure)?

- **Operational Omissions**
  - Logging and monitoring considerations?
  - Rollback strategy if deployment fails?
  - Database migration handling?
  - Feature flag or gradual rollout?

- **Testing Omissions**
  - How will this be tested?
  - Are there specific test cases mentioned?
  - Integration/E2E testing considerations?

- **Documentation Omissions**
  - Will CLAUDE.md need updates?
  - API documentation changes needed?
  - User-facing documentation impacts?

### 3.4 Problem Not Fully Addressed

Evaluate completeness of the solution:

- **Partial Solutions**
  - Does it fix the symptom but not the root cause?
  - Are there related issues that will still exist?
  - Does it create technical debt?

- **Scope Gaps**
  - Are all affected areas identified?
  - Are downstream impacts considered?
  - Are there related features that need similar changes?

- **Regression Risks**
  - Could this break existing functionality?
  - Are there callers/consumers not considered?
  - Are backward compatibility requirements met?

## 4. Severity Assessment

For each issue found, assess severity:

- **CRITICAL**: Plan will fail or cause significant problems if implemented as-is
- **HIGH**: Significant risk of bugs, security issues, or user impact
- **MEDIUM**: Could cause issues in certain scenarios
- **LOW**: Minor improvement opportunity or edge case

## 5. Output Format

```
## Plan Critique Report

**Plan File**: [filename]
**Critique Date**: [date]
**Overall Risk Level**: [CRITICAL/HIGH/MEDIUM/LOW]

### Executive Summary
[2-3 sentence summary of the most important findings]

### Issue Summary

| Severity | Count |
|----------|-------|
| Critical | X |
| High | X |
| Medium | X |
| Low | X |

### Critical Issues

#### [Issue Title]
- **Category**: [Edge Case/Misunderstanding/Omission/Incomplete]
- **Description**: [What the issue is]
- **Risk**: [What could go wrong]
- **Recommendation**: [How to address it]

### High Severity Issues

#### [Issue Title]
- **Category**: [Edge Case/Misunderstanding/Omission/Incomplete]
- **Description**: [What the issue is]
- **Risk**: [What could go wrong]
- **Recommendation**: [How to address it]

### Medium Severity Issues

#### [Issue Title]
- **Category**: [Edge Case/Misunderstanding/Omission/Incomplete]
- **Description**: [What the issue is]
- **Recommendation**: [How to address it]

### Low Severity Issues
- [Brief description] - Recommendation: [quick fix]

### Questions to Consider
[List of questions the plan author should answer before proceeding]

### Suggested Plan Amendments
[Specific additions or changes to strengthen the plan]
```

## 6. Guidelines for Critique

- Be constructive, not destructive - the goal is to improve the plan
- Focus on genuine risks, not hypothetical impossibilities
- Consider the SureDrop context: multi-tenant, security-focused, legacy codebase
- Check if the plan follows patterns established in CLAUDE.md
- Consider both the immediate change and ripple effects
- If the plan looks solid, say so - don't invent issues
- Prioritize issues that would cause real problems over theoretical concerns

## Notes

- If no `~/.claude/plans/` directory exists, suggest creating one
- If a plan name is provided, use that specific plan
- This critique should be done BEFORE implementation begins
- The critique is advisory - the plan author makes final decisions
- Focus on factual analysis and evidence-based concerns
