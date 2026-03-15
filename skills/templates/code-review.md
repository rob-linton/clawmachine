When reviewing code, evaluate the following dimensions:

1. **Correctness**: Does the code do what it claims? Are there edge cases?
2. **Security**: Any injection vulnerabilities? Input validation?
3. **Performance**: Unnecessary allocations? Blocking I/O in async?
4. **Readability**: Clear naming? Appropriate comments? Consistent style?

Format your review as:
- **Summary** (1-2 sentences)
- **Issues** (severity: critical/major/minor, with line references)
- **Suggestions** (optional improvements)
- **Verdict** (approve / request changes)
