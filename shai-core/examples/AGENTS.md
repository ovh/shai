# AGENTS.md

## Project Overview

Shai is a terminal-based coding agent written in Rust. It provides an interactive CLI, headless mode, and an HTTP server with OpenAI-compatible APIs. Shai supports multiple LLM providers, MCP integration, skills, and a shell assistant.

## Repository Structure

- **`shai-cli/`** — CLI entry point (TUI, headless mode, shell assistant).
- **`shai-core/`** — Core library: agent loop, state machine, tools, skills, memory.
- **`shai-llm/`** — LLM provider wrappers (OpenAI, Anthropic, Mistral, OVHCloud, Ollama, OpenRouter).
- **`shai-http/`** — HTTP server with OpenAI-compatible endpoints and SSE streaming.
- **`shai-macros/`** — Procedural macros (`#[tool]` attribute).
- **`docs/`** — Additional documentation.
- **`tests/`** — Integration and unit tests.
- **`examples/`** — Small example programs.

## Build Commands

```bash
# Debug build
cargo build

# Release build
cargo build --release

# Run tests
cargo test

# Run clippy
cargo clippy

# Format code
cargo fmt
```

### Package names

The workspace crates have different package names than their directory names:

| Directory       | Package name   |
| --------------- | -------------- |
| `shai-cli/`    | `shai`         |
| `shai-core/`   | `shai-core`    |
| `shai-llm/`    | `shai-llm`     |
| `shai-http/`   | `shai-http`    |
| `shai-macros/` | `shai-macros`  |

When using `-p` with cargo commands, always use the package name (e.g., `cargo build -p shai`), not the directory name.

## Code Style

- Follow Rust idioms: `cargo fmt` and `cargo clippy` must pass.
- Use `#[tool]` macro for defining new tools (see `shai-macros/`).
- Unit tests live in `#[cfg(test)] mod tests` at the bottom of each file.
- Integration tests go in the `tests/` directory.
- New tools must be registered in `shai-core/src/tools/mod.rs` and `shai-core/src/agent/builder.rs`.

## Architecture Notes

- The agent loop is in `shai-core/src/agent/` — it manages conversation state, tool execution, and LLM calls.
- Tools are defined as structs implementing the `Tool` trait (`shai-core/src/tools/types.rs`).
- Skills are discovered from `.shai/skills/` (project-local) and `~/.config/shai/skills/` (global). Each skill is a directory with a `SKILL.md` file.
- Project context is loaded from `AGENTS.md` (canonical) and `SHAI.md` (legacy override) at the git root.
- Memory facts are stored in `.shai/memory.md`.

## Testing

- Run all tests: `cargo test`
- Run tests for a specific crate: `cargo test -p shai-core`
- Run a single test: `cargo test -p shai-core <test_name>`
- Use `tempfile::TempDir` for filesystem tests.
- Use `mockito` for HTTP mocking.

## Release Process

1. Update the version in all `Cargo.toml` files (`shai-cli`, `shai-core`, `shai-llm`, `shai-macros`).
2. Run `cargo check` and `cargo test` to verify.
3. Commit with `chore: release vX.Y.Z`.
4. Tag with `vX.Y.Z` and push.
5. Verify the CI/CD pipeline picks up the tag.

## Contributing

- Submit changes via GitHub Pull Requests.
- Follow DCO sign-off (`Signed-off-by: Name <email>`).
- New files must include the Apache 2.0 license header.
- Code must be unit-tested and documented.
