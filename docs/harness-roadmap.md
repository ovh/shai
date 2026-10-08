# Roadmap: AI Agent Ecosystem Harness Improvements for shai

## Context

shai is a focused, lean terminal coding agent (Rust, ~24k LOC, OVH). Its core
(async actor agent, capability/permission model, MCP, multi-provider, OpenAI-compatible
HTTP server) is sound, but it lags the broader agent ecosystem on **cost/token
efficiency**, **interoperability with emerging standards**, and **editing precision**.

This roadmap consolidates findings from a review of four reference tools and two
emerging conventions, mapped onto shai's actual code:

- **rtk** (CLI output-compression proxy) → tool-output token reduction
- **dirac** (TS agent) → hash-anchored edits, AST-native edits, multi-file batching
- **goose** (Block/linux-Foundation, also Rust) → multi-model routing, recipes, evals
- **AGENTS.md** (cross-tool standard) and **Skills (SKill.md)** conventions

**Objective:** bring ecosystem-standard harness improvements to shai while preserving
its lean, embeddable, terminal-first identity (do *not* grow into a general
desktop agent like goose).

---

## Completed

### Cross-cutting: measurement ✅
Token tracking is fully implemented:
- Per-LLM-call tracking via `ThinkerDecision.token_usage` (`shai-core/src/agent/brain.rs`).
- Cumulative tracking in the TUI (`total_input_tokens`, `total_output_tokens`, `total_cached_tokens`).
- Per-tool compaction bytes (`original_bytes`/`compacted_bytes` in `ToolCallCompleted` event).
- `/tokens` slash command displays full breakdown; status bar shows totals.

### 1.3 Tool-output compaction ✅
Bash-aware compaction (`bash_aware.rs`), generic ANSI strip + dedup (`generic.rs`),
progressive + hard trace compaction (`trace.rs`). Compaction runs before tool
results enter the trace (`shai-core/src/agent/actions/tools.rs:254-338`).

### 2.1 AGENTS.md support ✅
Loads `AGENTS.md` from git root (`shai-core/src/runners/coder/prompt.rs:135`),
`SHAI.md` demoted to legacy override. Git-root discovery via `find_git_root()`
(`shai-core/src/runners/coder/env.rs:157`).

### 2.2 Skills (SKILL.md) support ✅
Discovery from `.shai/skills/` + `~/.config/shai/skills/` (`discovery.rs`),
catalog injected into system prompt, `SkillTool` loads body on demand.
Registered in `coder()` tool list (`coder.rs:254`).

### 2.3 Config import from .claude / .cursor ✅
`shai import [--overwrite]` detects `CLAUDE.md`, `.claude/`, `.cursorrules`,
`.cursor/rules` and converts to `AGENTS.md` (`shai-cli/src/import.rs`).

### 3.1 Multi-file batch edits ✅
`EditTool` accepts `Vec<FileEdit>`, applies atomically in memory before writing
to disk (`shai-core/src/tools/fs/edit/edit.rs`).

### 3.2 Hash-anchored edits ✅
Read tool emits stable per-line hashes (`hash.rs`), Edit tool accepts
`line_hash`/`insert_after_hash` anchors (`shai-core/src/tools/fs/edit/structs.rs`).

### 3.3 Post-edit verification feedback loop ✅
Runs language-specific checks (`cargo check`, `go build`, `python -m py_compile`)
after edits (`shai-core/src/tools/fs/verification.rs`). Disabled by default
(`VerificationConfig.enabled = false`).

### 3.4 AST / tree-sitter foundation ✅
Symbol outlining (`tree-sitter-tags`) and syntax highlighting (`tree-sitter-highlight`)
for 14+ languages. Used by Read tool (`outline: true`). Actively wired into
`shai-core/src/tools/fs/symbol/` and `tools/highlight.rs`.

---

## Remaining Work

### 1.1 Prompt caching *(source: generic / Anthropic best practice)*
**Goal:** stop re-paying for the stable prefix (system prompt + tool schemas + trace)
every turn (~90% input-token discount on cache hits).

**Status:** Not started.

**Provider split (critical constraint):**
- **Anthropic** (native JSON) and **Mistral** (already on `ChatClient` + `JsonHooks`)
  can inject `cache_control` cleanly — **target Anthropic first**.
- openai/ovhcloud/ollama/openrouter/openai_compatible go straight through
  `openai_dive`'s `client.chat().create()` with no JSON intervention → **deferred
  to backlog** (needs a wrapper or upstream support).

**Key files / insertion points:**
- `shai-llm/src/providers/anthropic/anthropic.rs` — `convert_to_anthropic_format()`
  (line 194): add `"cache_control": {"type":"ephemeral"}` breakpoints.
  Request posted at lines 448-455.
- `shai-llm/src/chat.rs` — `JsonHooks` trait (lines 17-35), `before_send()`.
  Create a `CacheControlHooks` impl; Mistral's `MistralHooks::before_send`
  (`providers/mistral.rs:89-98`) is the working reference pattern.
- **Breakpoint strategy:** after system prompt + tool defs (stable), and after
  the last stable trace message (moving cache).

**Verify:** run a multi-turn Anthropic session; assert `cache_read_input_tokens > 0`
on turn ≥2 via the measurement counters.

### 1.2 Retry / backoff / rate-limit handling — partial ✅
**Goal:** survive transient 429/500/503/529 + honor `Retry-After` instead of dying
mid-run.

**Status:** Core retry logic implemented (`shai-llm/src/client.rs:288-314`).
Exponential backoff with configurable max retries (`SHAI_LLM_MAX_RETRIES`).

**What remains:**
- Parse `Retry-After` header from 429/503/529 responses and use it as the delay
  instead of exponential backoff when present.
- Currently `is_retryable_error()` (line 275) retries on ALL errors — consider
  restricting to transient error types.

**Key files:**
- `shai-llm/src/client.rs` — retry loop at lines 288-314 (chat) and 326-351 (chat_stream).
- `shai-llm/src/chat.rs` — `check_status_code()` at line 96 maps HTTP codes.

**Verify:** unit test with a mock returning 429 + `Retry-After` header, then 200.

### Memory tool wiring *(from 2.1 Memory Tier 1)*
**Goal:** register the existing `MemoryWriteTool`/`MemoryReadTool` so the agent
can persist facts across sessions.

**Status:** Tools exist (`shai-core/src/tools/memory/memory.rs`) but are not
registered in the agent's tool list.

**What remains:**
- Register `MemoryWriteTool` and `MemoryReadTool` in `coder()` factory tool list
  (`shai-core/src/runners/coder/coder.rs:244-255`).
- Add `MemoryRead`/`MemoryWrite` variants to the headless `ToolName` enum
  (`shai-cli/src/headless/tools.rs`): `all()`, `name()`, `from_str()`.

**Verify:** agent can write a memory fact and read it back in the same session
without restart.

### `--restore` without args → session picker
**Goal:** `shai --restore` (no session ID) should launch the interactive
`SessionPicker` directly, instead of requiring a session ID argument.

**Status:** `--restore <ID>` works; `--latest` works. Bare `--restore` does nothing.

**Key files:**
- `shai-cli/src/main.rs` (lines 260-273) — currently `cli.restore` is `Option<String>`,
  `None` falls through silently.
- `shai-cli/src/tui/app.rs` — `restore_session()` and `handle_session_picker_key()`
  already exist.

**Approach:** When `--restore` is passed without a value, set a flag and launch
the TUI with the session picker open immediately.

---

## Deferred

### 2.4 Memory Tier 2
Layered/merged context across global + project + nested, live reload, richer
memory store. Builds directly on 2.1. Retrieval/embeddings memory is explicitly
out of scope (ceiling).

### Headless `--restore`/`--latest`
TUI has full session restore support (`--restore <ID>`, `--latest`, `Ctrl+O`
picker). Headless mode does not support session restore. Needs design work to
determine how traces are loaded and replayed in non-interactive contexts.

### Multi-model (lead/worker) routing *(goose)*
Strong "lead" model plans, cheap "worker" model executes. Confirmed gap
(shai is single-model). Major cost lever; `Brain`/`ToolCallMethod` already
abstracted (`agent/agent.rs:65`). Strongest candidate to promote once Phase 1
lands.

### Prompt caching for openai_dive providers
Wrapper layer so caching isn't Anthropic-only (follow-up to 1.1).

### Recipes + scheduler *(goose)*
Parameterized, shareable, schedulable headless workflows (distinct from skills).
Fits shai's automation/headless spot.

### Subagent context isolation
A delegate tool (e.g. expose `searcher` as dispatchable) returning a digest to
keep the main trace lean.

### Bash hard sandbox
Landlock/seccomp on Linux as defense-in-depth atop `ClaimManager` (bash
currently runs unsandboxed: `tools/bash/bash.rs:137`).

### Eval harness *(goose)*
Fixed tasks → outcome assertions, to measure whether these changes help.
Pairs with CI hygiene fixes below.

---

## Hygiene (independent, low-effort)

### CI improvements
- Add `cargo test` / `clippy -D warnings` / `fmt --check` to CI (`.github/workflows/ci.yml`).
- Remove crate-wide `dead_code`/`unused_variables` lint suppression.
- Replace fragile string-parsing in the `#[tool]` macro (`shai-macros/src/lib.rs:25-59`)
  with `syn`.

---

## Suggested sequencing

```
1. Memory tool wiring (quick win)
2. --restore without args (quick win)
3. 1.2 Retry-After header parsing
4. 1.1 Prompt caching (Anthropic)
5. Promote from backlog as capacity allows (multi-model routing is strongest next cost lever)
```

## Overall verification
- **1.1:** multi-turn Anthropic session shows cache reads + measurable input-token drop.
- **1.2:** injected-429-with-Retry-After test recovers; snapshot tests show reduction with errors intact.
- **Memory:** agent writes a fact and reads it back same session.
- **`--restore`:** `shai --restore` launches the picker; selecting a session restores it.
- **Phase 2/3 items:** already verified — no changes needed.
