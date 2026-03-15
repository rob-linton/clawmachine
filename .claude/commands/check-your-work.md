# Self-Review and Quality Assurance Check

Perform a comprehensive, deep-thinking review of all work completed in this session. This is a thorough quality assurance pass that must be completed before considering any task finished.

## Instructions

Use maximum available thinking tokens. Take your time. Be thorough and methodical.

## 1. Incomplete Implementation Detection

Scan ALL modified files for:

### Placeholders
- `TODO`, `FIXME`, `XXX`, `HACK`, `BUG` comments
- `TBA`, `TBD`, `PLACEHOLDER` markers
- Placeholder values like `"placeholder"`, `"TODO"`, `"xxx"`, `0` or `-1` used as sentinels
- Empty function/method bodies that should have implementation
- `pass`, `...`, `unimplemented!()`, `todo!()` statements
- Comments indicating future work: "will implement", "to be added", "not yet", "coming soon"

### Stub Code
- Functions that only `throw UnimplementedError()` or equivalent
- Methods returning hardcoded dummy data
- Conditional branches with `// handle this case` comments
- Error handlers that just `print()` or log without proper handling
- API calls with hardcoded URLs or credentials that should be configurable

### Missing Pieces
- Imported but unused dependencies (may indicate planned but unimplemented features)
- Declared but unimplemented interfaces/abstract methods
- Switch/match statements missing cases
- Commented-out code blocks that look like they should be active

## 2. Logic and Correctness Review

### Error Handling
- Are all error paths handled appropriately?
- Are there any swallowed exceptions (empty catch blocks)?
- Do async operations have proper error handling?
- Are error messages clear and actionable?

### Edge Cases
- Null/empty/undefined handling
- Boundary conditions (0, negative, max values)
- Empty collections
- Concurrent access issues
- Network failure scenarios

### Type Safety
- Any use of `dynamic`, `any`, `Object` that should be typed?
- Unsafe casts that could fail at runtime?
- Optional/nullable access without null checks?

## 3. Consistency Check

### Code Style
- Does new code match existing patterns in the codebase?
- Are naming conventions consistent?
- Is indentation/formatting consistent with surrounding code?

### Architecture
- Does the implementation follow established patterns?
- Are there any violations of the project's layering/structure?
- Is state management consistent with the rest of the app?

### API Contracts
- Do request/response formats match documentation?
- Are all required fields present?
- Do error codes/messages match the specification?

## 4. Security Review

- No hardcoded secrets, passwords, or API keys
- Proper authentication checks on protected endpoints
- Input validation on user-provided data
- SQL/Command injection prevention
- XSS prevention in any HTML output
- Proper authorization (not just authentication) checks

## 5. Documentation Sync

### CLAUDE.md Verification
Read CLAUDE.md and verify:
- All new Redis keys or database tables are documented in the key hierarchy
- All new API endpoints are documented
- Any new architectural patterns are explained
- Configuration options are documented
- The documentation accurately reflects current implementation

### Code Comments
- Are complex algorithms explained?
- Are non-obvious business rules documented?
- Are there any stale/outdated comments that contradict the code?

## 6. Test Considerations

- Are there obvious test cases that should exist?
- Does the implementation break any existing tests?
- Are there manual testing steps that should be documented?

## 7. Final Verification

After completing the review:

1. **List all issues found** with file paths and line numbers
2. **Categorize by severity**: Critical (must fix), Warning (should fix), Info (nice to fix)
3. **For each issue**, propose a specific fix
4. **Fix all Critical issues** before reporting completion
5. **Update CLAUDE.md** if any documentation gaps were found

## Output Format

```
## Review Summary

### Critical Issues (Must Fix)
- [ ] File:line - Description - Fix applied: Yes/No

### Warnings (Should Fix)
- [ ] File:line - Description - Fix applied: Yes/No

### Info (Nice to Fix)
- [ ] File:line - Description

### Documentation Updates
- [ ] CLAUDE.md section X updated: Yes/No
- [ ] Other docs updated: Yes/No

### Verification Complete
All critical issues resolved: Yes/No
Documentation in sync: Yes/No
```
