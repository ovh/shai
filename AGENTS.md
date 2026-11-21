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
- Skills are discovered from three tiers, in descending priority: `.shai/skills/` (project-local), `~/.config/shai/skills/` (global), and built-in skills embedded in the binary (registered in `shai-core/src/tools/skills/builtin.rs`, source in `shai-core/skills/` — currently only the generic `memory` skill). Shai/Rust-specific sample skills live in `shai-core/examples/skills/` and are not embedded. A skill name in a higher tier shadows the same name in lower tiers. Each skill is a directory with a `SKILL.md` file.
- Project context is loaded from `AGENTS.md` (canonical) and `SHAI.md` (legacy override) at the git root.
- **Model roles** (`shai-core/src/config/roles.rs`) are optional named secondary models in the auth config: `roles: { "<name>": "<provider>/<model>" }`. A role reuses the credentials of the `providers` entry it names. Resolution is synchronous and offline: `ShaiConfig::resolve_role` returns `Ok(None)` for a role that is not configured (feature off), `resolve_model_target` treats that as an error because the user asked for the model by name, and anything unresolvable is an error with no fallback to the primary model or the built-in default. `ResolvedModel::bind` yields a `BoundModel` (client plus model name); the client comes from `LlmClient::create_provider_isolated`, which never reads provider settings from the process environment. Never call `LlmClient::default_model` on it: that returns the primary model.
- **Prewalk** (`shai-core/src/agent/prewalk.rs`) is a one-shot, mid-session handoff to another model, armed with `--prewalk-into <role-or-provider/model>` or `prewalk.into` in the auth config. Once a `todo_write` has left a non-empty list and a built-in `write`/`edit` call has succeeded (`PrewalkSignals`, recorded in `agent/actions/tools.rs`), `AgentCore::prewalk_if_due` swaps the brain's client and model through `Brain::switch_model` right before the next LLM call, and emits `AgentEvent::ModelSwitched`. The trace, todos and tool state are untouched. The handoff is a `Prewalk`: a target plus a latch shared by its clones, so that agents armed with the same one switch once between them. `AgentCore::arm_prewalk` refuses an agent that could never hand off (no built-in `todo_write`, neither `write` nor `edit`, or a brain whose `can_switch_model` is false); front-ends arm through `SessionOptions::arm` (`shai-cli/src/session_options.rs`) and show the line it returns. A target that cannot be resolved at startup, or a config file that exists and cannot be read, is an error (exit 2); the target is resolved again for every agent the TUI starts (`SessionOptions::refresh`), and an agent started after the handoff is told apart with a `prewalk is over` line. Optional config sections such as `prewalk` are wrapped in `config::lenient::Lenient`, on top of `config::section::RawSection`, so that a wrong value in them, or a section written twice, cannot fail the whole config load; `ShaiConfig` is (de)serialized by hand for that reason, so a new optional section needs a line in both impls.
- Memory uses an index + topic-file layout. Each scope has a `MEMORY.md` index (one timestamped line per fact) plus detail topic files: global at `~/.config/shai/memory/`, project-local at `.shai/memory/`. The `memory_write`/`memory_remove` tools manage the index only; the `memory` built-in skill defines the protocol for topic files. Indexes are capped at 200 lines / 25KB and injected into the system prompt via the `{{MEMORY}}` placeholder.
- **Plan mode** denies all tool execution (read-only). It is enforced both by a dedicated system prompt (`PLAN_MODE_PROMPT` in `shai-core/src/runners/coder/prompt.rs`) that instructs the LLM to only plan, and by `ClaimManager` which blocks all write tools when `is_plan_mode` is set. Managed via `AgentRequest::PlanMode` / `AgentResponse::PlanModeStatus`.
- Configs can be imported from external tools (Claude `CLAUDE.md`, Cursor `.cursorrules`/`.cursor/rules`) into `AGENTS.md` via the `import` module (`shai-cli/src/import.rs`).
- The TUI includes a **session picker** (`shai-cli/src/tui/session_picker.rs`) for browsing and restoring saved sessions.
- The TUI **status bar** (`shai-cli/src/tui/statusbar.rs`) displays model, provider, working directory location, git branch, agent mode, and token counts. It is updated via `StatusBar` setters (`set_location`, `set_git_branch`, `set_agent_mode`, `set_tokens`).

## Testing

- Run all tests: `cargo test`
- Run tests for a specific crate: `cargo test -p shai-core`
- Run a single test: `cargo test -p shai-core <test_name>`
- Use `tempfile::TempDir` for filesystem tests.
- Use `mockito` for HTTP mocking.

## Release Process

1. Update the version in all `Cargo.toml` files (`shai-cli`, `shai-core`, `shai-llm`, `shai-http`, `shai-macros`).
2. Run `cargo check` and `cargo test` to verify.
3. Commit with `chore: release vX.Y.Z`.
4. Tag with `vX.Y.Z` and push.
5. Verify the CI/CD pipeline picks up the tag.

## Contributing

- Submit changes via GitHub Pull Requests.
- Follow DCO sign-off (`Signed-off-by: Name <email>`).
- New files must include the Apache 2.0 license header.
- Code must be unit-tested and documented.
