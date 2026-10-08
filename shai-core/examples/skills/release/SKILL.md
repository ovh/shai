---
name: release
description: Prepare and publish a new release of the SHAI project, including version bumps and tagging
---

# Release

## Purpose
Guide the release process for SHAI, ensuring all version numbers are updated and the release is properly tagged.

## Release Procedure

### 1. Determine the Version
- Check the current version: `grep '^version' shai-cli/Cargo.toml | head -1`
- Decide on the new version number following [Semantic Versioning](https://semver.org/):
  - **Patch** (`x.y.Z`): Bug fixes only.
  - **Minor** (`x.Y.0`): New features, backward-compatible.
  - **Major** (`X.0.0`): Breaking changes.

### 2. Update Version Numbers
Update the version in **all** crate manifests:
1. `shai-cli/Cargo.toml`
2. `shai-core/Cargo.toml`
3. `shai-llm/Cargo.toml`
4. `shai-macros/Cargo.toml`

### 3. Verify the Build
```bash
cargo check
cargo test
cargo build --release
```

### 4. Commit and Tag
```bash
git add -A
git commit -m "chore: release v<VERSION>"
git tag v<VERSION>
git push origin main --tags
```

### 5. Verify Release Workflow
- Check that the CI/CD pipeline picks up the tag and starts the release workflow.
- Verify the binary is published to GitHub Releases.

## Post-Release
- Update any documentation references to the new version.
- Announce the release in relevant channels.
- Create a draft GitHub Release with release notes summarizing changes since the last release.

## Guidelines
- Never skip the `cargo check` and `cargo test` steps.
- Ensure the working tree is clean before tagging.
- If something goes wrong, delete the tag immediately: `git tag -d v<VERSION>` and `git push origin :v<VERSION>`.
- Keep release notes user-facing — mention notable features, fixes, and breaking changes.
