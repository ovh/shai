---
name: rust-lint
description: Check and fix Rust build warnings and clippy warnings across the project
---

# Rust Lint

## Purpose
Run `cargo build` and `cargo clippy` to surface compiler and linter warnings, then fix them systematically.

## Procedure

### 1. Gather Warnings
Run both commands and capture all output:

```bash
cargo build 2>&1
cargo clippy --all-targets 2>&1
```

If the project uses a workspace, warnings will cover all crates. If you only want to lint a specific crate:

```bash
cargo clippy -p <crate-name> --all-targets 2>&1
```

### 2. Categorize Warnings
Group warnings into categories:
- **Unused imports / variables**: Remove or prefix with `_`.
- **Dead code**: Remove unused functions, structs, or fields. If they are part of a public API, document them with `#[allow(dead_code)]` only if genuinely needed.
- **Needless lifetimes**: Remove explicit lifetime annotations that the compiler can infer.
- **Clippy style warnings**: Apply the suggested fix (e.g., `unwrap_or_default()`, `is_empty()`, `len() == 0` → `is_empty()`).
- **Missing documentation**: Add doc comments to public items.
- **Improper regex / format strings**: Fix as suggested.

### 3. Fix Warnings
For each warning:
1. Read the surrounding code using the `read` tool to understand context.
2. Apply the fix using `edit` or `multiedit`.
3. Re-run the specific check to confirm the warning is resolved:

```bash
cargo clippy -p <crate-name> --all-targets 2>&1 | grep -A 5 "<warning pattern>"
```

### 4. Verify
After all fixes:
- Run `cargo build` and confirm no warnings remain.
- Run `cargo clippy --all-targets` and confirm no warnings remain.
- Run `cargo test` to ensure nothing broke.
- Run `cargo fmt --check` to ensure formatting is clean.

## Guidelines
- Fix one category of warnings at a time to keep changes reviewable.
- Never suppress a warning with `#[allow(...)]` unless there is a justified reason — document it in a comment.
- Prefer clippy's suggested fixes when available.
- If a warning is a false positive, add a targeted `#[allow(...)]` with a comment explaining why.
- Keep commits focused: one logical fix per commit (e.g., "fix: remove unused imports").
- Do not introduce new dependencies to silence warnings.
- If a warning originates from a dependency, consider upgrading or ignoring it via `clippy.toml` or `#![allow]` at the crate level.
