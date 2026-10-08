---
name: repo-analysis
description: Analyze uncommitted changes in the repository, group them into logical commits, and determine the appropriate version bump
---

# Repository Change Analysis

## Purpose
Analyze uncommitted changes in the repository, group them into logical commits, determine the appropriate version bump, and guide the user through committing each group cleanly.

## Procedure

### 1. Gather All Changes
- Run `git status` to see the full list of modified, staged, deleted, and untracked files.
- Run `git diff` and `git diff --staged` to inspect the actual changes.
- If the diff is large, review files one by one using `git diff -- <file>` or `git diff --staged -- <file>`.

### 2. Identify Logical Change Groups
Analyze the changes and group them into logical features/fixes. Each group should:
- Represent a single coherent concern (e.g., "add TUI status bar", "refactor session persistence", "fix pty leak").
- Be independently committable without breaking the build.
- Map to a conventional commit type (`feat:`, `fix:`, `refactor:`, `docs:`, `test:`, `chore:`, `ci:`).

Present the proposed grouping to the user in a structured format:

```
## Proposed Commits

### Commit 1 — feat: <short description>
- **Files:** list of files
- **Rationale:** why these belong together

### Commit 2 — fix: <short description>
- **Files:** list of files
- **Rationale:** why these belong together

...
```

Wait for user confirmation before proceeding.

### 3. Determine Version Bump
Based on the full set of changes, determine the appropriate version bump following [Semantic Versioning](https://semver.org/):

| Change Type | Version Bump | Examples |
|-------------|---------------|---------|
| **Major** (`X.0.0`) | Breaking changes, API removals, incompatible behavioral changes | Renaming/removing public APIs, changing config format, removing features |
| **Minor** (`x.Y.0`) | New features, backward-compatible additions | New tools, new commands, new config options, new capabilities |
| **Patch** (`x.y.Z`) | Bug fixes, refactors, docs, chores | Fixes, code cleanup, dependency updates, documentation |

#### Decision Guidelines
- If **any** change introduces a breaking change → **major** bump.
- Else if **any** change adds a new feature → **minor** bump.
- Else → **patch** bump.
- When in doubt, prefer the **lower** bump unless the user specifies otherwise.

Present the recommendation:
```
## Version Recommendation
- **Current version:** x.y.z
- **Proposed version:** x.y.z+1 (or X+1.0.0 / x.Y+1.0)
- **Bump type:** major/minor/patch
- **Rationale:** <brief explanation>
```

Wait for user confirmation before proceeding.

### 4. Stage and Commit Each Group
For each logical change group:
1. Stage the relevant files: `git add <files>` (or `git add -p` for partial staging).
2. Commit with a clear conventional-commit message:
   ```bash
   git commit -m "feat: short description" -m "Detailed explanation of what and why."
   ```
3. Verify the commit succeeded with `git log --oneline -1`.

### 5. Update Version Numbers
If a version bump is needed:
1. Check the current version: `grep '^version' shai-cli/Cargo.toml | head -1`
2. Update the version in **all** crate manifests:
   - `shai-cli/Cargo.toml`
   - `shai-core/Cargo.toml`
   - `shai-llm/Cargo.toml`
   - `shai-macros/Cargo.toml`
   - Also update `Cargo.lock` with `cargo check` or `cargo build`.
3. Commit the version bump:
   ```bash
   git add -A
   git commit -m "chore: release vX.Y.Z"
   ```

### 6. Final Verification
- Run `cargo fmt` and `cargo clippy` to ensure code quality.
- Run `cargo test` to verify everything compiles and passes.
- Show the final `git log --oneline` to confirm the commit history is clean.

## Guidelines
- **Never** commit secrets, credentials, or `.env` files.
- **Always** confirm the grouping and version bump with the user before committing.
- Keep commit messages under 72 characters for the subject line.
- Use the imperative mood in commit messages (e.g., "Add feature X", not "Added feature X").
- If changes are interdependent, note the order in which commits should be applied.
- If a single file contains changes belonging to multiple groups, use `git add -p` to stage hunks selectively.
- Do not push unless explicitly asked by the user.
- If the working tree has merge conflicts, resolve them before attempting to commit.
