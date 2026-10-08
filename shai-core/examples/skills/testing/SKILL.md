---
name: testing
description: Write and run tests, ensure adequate test coverage, and fix failing tests
---

# Testing

## Purpose
Create, run, and maintain tests for the SHAI project. Ensure new code is tested and existing tests pass.

## Running Tests

```bash
# Run all tests
cargo test

# Run tests for a specific crate
cargo test -p shai-core

# Run a specific test module
cargo test -p shai-core tools::skills::discovery

# Run a single test function
cargo test -p shai-core test_parse_skill_frontmatter_with_metadata

# Run tests with output
cargo test -- --nocapture

# Run only unit tests (no integration)
cargo test --lib

# Run doc tests
cargo test --doc
```

## Test Conventions

### Structure
- Place unit tests in `#[cfg(test)] mod tests` at the bottom of the file they test.
- Place integration tests in the `tests/` directory.
- Name tests descriptively: `test_<what>_<condition>` (e.g., `test_strip_frontmatter_with_metadata`).

### Coverage Checklist
When writing or reviewing tests, ensure coverage for:
- **Happy path**: Normal expected behavior.
- **Edge cases**: Empty input, boundary values, single-element collections.
- **Error cases**: Invalid input, missing files, permission errors.
- **Integration**: Interactions between modules.

### Mocking
- Use `mockito` or similar for HTTP mocking when needed.
- Use `tempfile::TempDir` for filesystem tests.
- Avoid testing implementation details — test behavior, not internals.

## Writing Good Tests

```rust
#[test]
fn test_<descriptive_name>() {
    // Arrange — set up the test data
    let input = ...;

    // Act — perform the operation
    let result = function_under_test(input);

    // Assert — verify the outcome
    assert_eq!(result, expected);
}
```

## Guidelines
- Each test should verify **one** behavior.
- Test names should read like a specification: `test_<subject>_<expected_behavior>`.
- Prefer many small focused tests over one large test.
- Use `assert!` for boolean conditions, `assert_eq!` for equality.
- Always clean up resources (use `TempDir` or `Drop` implementations).
- If a test is flaky, fix it — don't disable it without a tracking issue.
