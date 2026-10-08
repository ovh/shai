---
name: git-workflow
description: Manage git branches, commits, rebases, and pull requests following project conventions
---

# Git Workflow

## Purpose
Handle common git operations cleanly and safely, following the project's branching and commit conventions.

## Commit Conventions
- Write clear, concise commit messages in the imperative mood (e.g., "Add feature X", not "Added feature X").
- Prefix commits with a type tag when applicable: `feat:`, `fix:`, `docs:`, `refactor:`, `test:`, `chore:`, `ci:`.
- Keep the subject line under 72 characters.
- Add a blank line after the subject line, then a detailed body explaining **what** and **why** (not **how**).

## Branch Naming
- Use descriptive branch names: `feat/<name>`, `fix/<name>`, `chore/<name>`.
- Use lowercase kebab-case.

## Procedure

### Before Committing
1. Run `git status` and `git diff --staged` to review what will be committed.
2. Ensure no debug prints or temporary files are staged.
3. Run `cargo fmt` and `cargo clippy` before committing.

### Creating a Commit
```bash
git add -p   # stage specific hunks interactively
git commit -m "feat: short description" -m "Detailed explanation of the change."
```

### Rebasing
- Always rebase onto the target branch before opening a PR:
  ```bash
  git fetch origin
  git rebase origin/main
  ```
- Resolve conflicts file by file, then `git rebase --continue`.

### Opening a Pull Request
- Push the branch: `git push -u origin <branch-name>`.
- Use a descriptive PR title and description.
- Link any related issues.

## Safety Rules
- **Never** force-push to `main`.
- **Never** commit secrets or credentials.
- Always confirm with the user before pushing or force-pushing.
- Use `--force-with-lease` instead of `--force` when force-pushing.
