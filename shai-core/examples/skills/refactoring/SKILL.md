---
name: refactoring
description: Restructure existing code to improve readability, maintainability, and performance without changing behavior
---

# Refactoring

## Purpose
Improve code quality without changing external behavior. Ensure refactoring is safe and verifiable.

## Before You Start
1. **Ensure tests pass**: Run `cargo test` to establish a baseline.
2. **Understand the code**: Read the code thoroughly before making changes.
3. **Plan the refactoring**: Identify what needs to change and why.

## Refactoring Techniques

### Extract Function
- Move inline logic into a named function when it has a clear single responsibility.
- Name the function after **what** it does, not **how** it does it.

### Simplify Conditionals
- Replace nested `if`/`else` chains with early returns or pattern matching.
- Use `match` instead of long `if`/`else if` chains when comparing against constants.
- Extract complex boolean expressions into named variables or helper functions.

### Reduce Duplication
- Identify repeated patterns and extract them into shared functions or macros.
- Be careful not to over-abstract — sometimes duplication is clearer than wrong abstraction.

### Improve Types
- Replace `String` with `&str` where ownership isn't needed.
- Use `enum` instead of string constants for known variants.
- Use `Option<T>` instead of sentinel values like empty strings.

### Module Restructuring
- Group related functionality into modules.
- Keep module boundaries clean — avoid circular dependencies.
- Use `pub(crate)` instead of `pub` when possible to limit API surface.

## Verification Checklist
After refactoring:
- [ ] `cargo test` passes
- [ ] `cargo clippy` has no new warnings
- [ ] `cargo fmt --check` passes
- [ ] No dead code introduced
- [ ] Public API is unchanged (or document breaking changes)

## Guidelines
- Make small, incremental commits — one logical change per commit.
- Prefer readability over cleverness.
- Don't mix refactoring with feature changes in the same commit.
- If unsure whether a change is safe, ask the user.
- Document any non-obvious design decisions in comments.
