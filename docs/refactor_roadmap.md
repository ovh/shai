# Refactoring Plan

## Context

`App` in `shai-cli/src/tui/app.rs` is a God Class — it manages terminal lifecycle, agent management, modal state, tool tracking, input handling, command history, permissions, token counting, UI theming, status bar, conversation history, session persistence, and tool output viewing all in one struct.

`AgentCore` (`shai-core/src/agent/agent.rs`) is also a God Object with 20 fields spanning agent state, LLM config, tool management, permissions, caching, session tracking, and event channels. The `handle_command` method alone is ~200+ lines.

The tool execution path (`spawn_tool_static`) takes 15 parameters, all cloned from `AgentCore` fields, indicating a missing abstraction.

## Current State

### `App` struct responsibilities

| Concern | Fields |
|---|---|
| Terminal lifecycle | `terminal` |
| Agent management | `agent`, `custom_agent` |
| Modal state | `state` (InputShown / PermissionModal) |
| Tool tracking | `running_tools`, `tool_start_times` |
| Input handling | `input` |
| Command history | `commands` |
| Permissions | `permission_queue` |
| Token counting | `total_input_tokens`, `total_output_tokens`, `total_cached_tokens` |
| UI theming | `theme` |
| Status bar | `status_bar` |
| Conversation history | `history` |
| Agent metadata | `agent_model`, `agent_provider`, `agent_name` |
| Session persistence | `session_id`, `last_assistant_response` |
| Tool output viewer | `last_tool_output`, `last_tool_file_path` |
| Session picker | `session_picker` |

### `AgentCore` communication layers

`AgentCore` has three distinct communication surfaces:

| Layer | Struct/Fields | Purpose |
|-------|---------------|---------|
| **External I/O** | `AgentSocket` (`tx_command`, `rx_command`, `tx_event`, `rx_event`) | Controller commands in, public events out |
| **Internal event bus** | `internal_tx` / `internal_rx` (broadcast channel) | Inter-coroutine plumbing (brain results, tool completion, permission responses) |
| **Tool execution context** | `spawn_tool_static` takes 15 params cloned from `AgentCore` fields | Shared state passed to tool coroutines |

## Proposed Refactoring

### Track A — TUI `App` extraction

**Files:** `shai-cli/src/tui/app.rs`, `shai-cli/src/tui/command.rs`, `shai-cli/src/tui/input.rs`, `shai-cli/src/tui/shortcuts.rs`

**Phase 1 — Field extractions (done):**

- [x] Extract `ToolTracker` struct (`running_tools`, `tool_start_times`, `last_tool_output`, `last_tool_file_path`).
- [x] Extract `TokenCounter` struct (`total_input_tokens`, `total_output_tokens`, `total_cached_tokens`).
- [x] Extract `SessionManager` struct (`session_id`, `last_assistant_response`).
- [x] Extract `PermissionManager` struct (`permission_queue`).

**Phase 2 — Structural refactoring (done):**

- [x] Introduce `Modal` trait (`draw()`, `handle_key_event()`) with `run_alternate_screen()` helper to deduplicate alternate-screen modal implementations (`AlternateScreenViewer`, `PromptPicker`, `AlternateScreenPermissionModal`).
- [x] Extract `CommandRegistry` from `command.rs` — slash commands no longer extend `App`.
- [x] Decompose `InputArea` into `FileSuggestion` and `CommandSuggestion` sub-components.

**Phase 3 — Cleanup (mostly done):**

- [x] Wire `shortcuts.rs` into the event loop — all TUI key bindings are now configurable via `tui.config.json`.
- [x] Config file migrations — `auth.config` → `auth.config.json`, `{name}.config` → `{name}.config.json` (legacy fallback).
- [x] `SHAI_KEY_*` env vars supported as fallback when `tui.config.json` doesn't exist.
- [x] `cancel_task`, `clear_input`, `paste` shortcuts moved from hardcoded `input.rs` checks to configurable `KeyBinding` fields.
- [x] Reduce `App` to pure orchestration between extracted components.

**Depends on:** Nothing.

### Track B — AgentCore field extraction

**Files:** `shai-core/src/agent/agent.rs`, `shai-core/src/agent/builder.rs`, `shai-core/src/agent/actions/tools.rs`, `shai-core/src/agent/states/*`

- [x] Extract `AgentSocket` (channel management: `tx_command`, `rx_command`) from `AgentCore` — already exists as a struct, formalized ownership. `tx_event`/`rx_event` moved to `ToolContext` as `public_event_tx`.
- [x] Extract `ToolCache` struct (`command_cache`, `read_cache`, `tool_call_metadata`) from `AgentCore`.
- [x] Extract `ToolBudget` struct (`tool_call_count`, `max_tool_calls`, `soft_tool_calls`) from `AgentCore`/`ThinkerContext`.
- [x] Move `tool_call_count`, `max_tool_calls`, `soft_tool_calls`, and `tool_call_metadata` off `ThinkerContext` — these are agent-loop concerns.
- [x] Introduce `ToolContext` struct to bundle the shared state passed to tool coroutines:

```rust
pub struct ToolContext {
    // Event channels
    pub public_event_tx: Option<broadcast::Sender<AgentEvent>>,
    pub internal_tx: broadcast::Sender<InternalAgentEvent>,
    // Shared state
    pub available_tools: Vec<Arc<dyn AnyTool>>,
    pub trace: Arc<RwLock<Vec<ChatMessage>>>,
    pub claims: Arc<RwLock<ClaimManager>>,
    pub compaction_config: CompactionConfig,
    pub verification_config: VerificationConfig,
    pub working_dir: Option<String>,
    pub todo_storage: Arc<TodoStorage>,
    pub fs_operation_log: Arc<FsOperationLog>,
    // Caching & budget
    pub tool_cache: ToolCache,
    pub tool_budget: ToolBudget,
}
```

- [x] Reduce `spawn_tool_static` from 15 parameters to 3 (`LlmToolCall` + `CancellationToken` + `ToolContext`).
- [x] Consolidate agent construction logic between `coder()` factory, `AgentBuilder::default()`, and `AppHeadless::run()`.

**Depends on:** Nothing.

### Deferred — Compaction rework

**Files:** `agent/agent.rs`, `agent/brain.rs`, `runners/coder/coder.rs`, `runners/compacter/*`

- Merge `addMaxTokenCompaction` branch into `refactorApp`.
- Introduce `CompactionManager` as described in `docs/rework_compact.md`.
- Move compaction logic out of `CoderBrain::next_step()`.
- [x] Simplify `ThinkerContext`: make `trace` an owned `Vec<ChatMessage>` snapshot instead of `Arc<RwLock<>>`.
- Remove `ContextCompressor` dead code references if any remain after merge.

**Depends on:** Tracks A and B (must settle `AgentCore` and `ThinkerContext` shape first).

## Parallelization

```
Track A (TUI):    [App extraction] ──────────────────────────► done
Track B (core):   [AgentCore + ToolContext extraction] ────────► done
                         │
                         ▼
                   [Compaction rework] ────────────────────────► done
```

- Tracks A and B are independent and can run in parallel.
- Compaction rework is deferred until both tracks settle.

## Priority

| Priority | Task | Effort | Risk |
|----------|------|--------|------|
| 1 | Track A — TUI App extraction | Medium | Low |
| 2 | Track B — AgentCore field extraction | Medium | Low |
| 3 | Deferred — Compaction rework | Medium | Low |
