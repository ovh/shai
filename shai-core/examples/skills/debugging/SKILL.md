---
name: debugging
description: Systematically diagnose and fix bugs using logs, stack traces, and targeted experiments
---

# Debugging

## Purpose
Provide a structured approach to diagnosing and fixing bugs in the SHAI codebase.

## Debugging Workflow

### 1. Reproduce the Issue
- Identify the exact command or action that triggers the bug.
- Note the expected vs. actual behavior.
- Check if the issue is reproducible or intermittent.

### 2. Gather Information
- Run with `RUST_BACKTRACE=1` to get full stack traces.
- Check logs for error messages or warnings.
- Use `git log --oneline -10` to see recent changes that might have introduced the bug.
- Run `git diff` to check for uncommitted changes.

### 3. Isolate the Problem
- Narrow down the failing component:
  ```bash
  # Run specific tests
  cargo test -p shai-core -- <test_name>
  
  # Run with backtrace
  RUST_BACKTRACE=1 cargo test -p shai-core -- <test_name> -- --nocapture
  ```
- Add temporary `dbg!()` or `eprintln!()` statements to trace execution flow.
- Use `cargo expand` to debug macro-generated code:
  ```bash
  cargo expand -p shai-core --tests 2>/dev/null | less
  ```

### 4. Formulate and Test Hypotheses
- State your hypothesis clearly: "The bug is caused by X because Y."
- Write a minimal reproduction test case.
- Fix the root cause, not just the symptom.
- Verify the fix doesn't break other tests: `cargo test`.

### 5. Verify the Fix
- Run the full test suite.
- Test edge cases around the fix.
- Check for similar patterns elsewhere in the codebase that might have the same bug.

## Common Issues in This Project

### Tool Registration Failures
- Check `shai-core/src/tools/mod.rs` for proper module declarations and re-exports.
- Verify the `#[tool]` macro is correctly applied.

### MCP Connection Issues
- Check the MCP configuration format.
- Verify the MCP server is running and accessible.
- Inspect the SSE/stdio transport layer for connection errors.

### Prompt Template Errors
- Check `shai-core/src/runners/coder/prompt.rs` for template variable mismatches.
- Ensure all `{{PLACEHOLDER}}` variables are properly resolved.

## Guidelines
- Always start with reproduction — a bug you can't reproduce is one you can't fix.
- Make minimal changes to fix the issue.
- Add a regression test to prevent the bug from recurring.
- Clean up any debug prints before committing.
