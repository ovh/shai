---
name: code-review
description: Review code changes in the current git branch, identify bugs, style issues, and suggest improvements
---

# Code Review

## Purpose
Review uncommitted or branch-diff changes for bugs, security issues, style violations, and potential improvements.

## Procedure

### 1. Gather Changes
- Run `git diff` to see unstaged changes, or `git diff main...HEAD` to see all changes on the current branch.
- If the diff is large, review it file by file using `git diff -- <file>`.

### 2. Analyze Each Changed File
For each file with changes:
- Read the full file context using the `read` tool to understand surrounding code.
- Check for:
  - **Correctness**: Logic errors, off-by-one errors, null/None handling.
  - **Security**: Injection vulnerabilities, unsafe deserialization, hardcoded secrets, improper input validation.
  - **Error Handling**: Missing error cases, panics, unwrap() in production paths.
  - **Style**: Consistency with existing codebase conventions (naming, formatting, patterns).
  - **Performance**: Unnecessary allocations, O(n²) loops, redundant clones.
  - **Dead Code**: Unused imports, unused variables, unreachable branches.

### 3. Check Tests
- Verify that new code paths are covered by tests.
- Ensure existing tests still pass with `cargo test`.
- Flag any untested public APIs.

### 4. Summarize Findings
Provide a structured summary:
```
## Review Summary

### Critical Issues
- [file:line] Description

### Suggestions
- [file:line] Description

### Positive Notes
- What was done well
```

## Guidelines
- Be constructive and specific — reference file names and line numbers.
- Distinguish between blocking issues and nice-to-haves.
- Respect existing conventions in the codebase.
- Do not modify files unless explicitly asked; suggest changes instead.
