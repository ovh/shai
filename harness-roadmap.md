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
- **goose** (Block/Linux-Foundation, also Rust) → multi-model routing, recipes, evals
- **AGENTS.md** (cross-tool standard) and **Skills (SKILL.md)** conventions

**Objective:** bring ecosystem-standard harness improvements to shai while preserving
its lean, embeddable, terminal-first identity (do *not* grow into a general
desktop agent like goose).

**Guiding decisions:**
- Phase 1 is the **cost/token efficiency** spine (highest ROI, lowest risk).
- The `.claude`/`.cursor` config import is a **lightweight note**, deferred (depends on AGENTS.md).
- rtk and dirac do **not** overlap: rtk = tool *output* compaction; dirac = *editing*.

A cost/efficiency spine runs through the whole plan: caching + compaction + (later)
multi-model routing all reduce tokens; measurement (below) validates each.

---

## Cross-cutting: measurement (do first, it gates everything)

Before/after token accounting so every Phase 1 claim is provable, not assumed
(rtk's `gain` metric; goose's evals philosophy).

- Reuse existing token tracking: `BrainResponse.token_usage` (`shai-core/src/agent/brain.rs:32`)
  and `AgentEvent::TokenUsage` (`shai-core/src/agent/events.rs:99-102`).
- Add: cache-hit/miss counters (cached vs uncached input tokens) and
  per-tool compaction bytes-in/bytes-out, surfaced in metadata + a summary at session end.

---

## Phase 1 — Cost & token efficiency (FIRST)

### 1.1 Prompt caching  *(source: generic / Anthropic best practice)*
**Goal:** stop re-paying for the stable prefix (system prompt + tool schemas + trace)
every turn (~90% input-token discount on cache hits).

**Provider split (critical constraint):**
- **Anthropic** (native JSON) and **Mistral** (already on `ChatClient` + `JsonHooks`)
  can inject `cache_control` cleanly — **target Anthropic first**.
- openai/ovhcloud/ollama/openrouter/openai_compatible go straight through
  `openai_dive`'s `client.chat().create()` with no JSON interception → **deferred to backlog** (needs a wrapper or upstream support).

**Key files / insertion points:**
- `shai-llm/src/providers/anthropic/anthropic.rs` — `convert_to_anthropic_format()`
  (lines 188-206): add `"cache_control": {"type":"ephemeral"}` breakpoints. Request posted at 448-455.
- `shai-llm/src/chat.rs` — `JsonHooks` trait (lines 17-35), `before_send()`. Create a
  `CacheControlHooks` impl; Mistral's `MistralHooks::before_send` (`providers/mistral.rs:89-98`)
  is the working reference pattern.
- **Breakpoint strategy:** after system prompt + tool defs (stable), and after the last
  stable trace message (moving cache).

**Verify:** run a multi-turn Anthropic session; assert `cache_read_input_tokens > 0` on
turn ≥2 via the measurement counters.

### 1.2 Retry / backoff / rate-limit handling  *(source: generic reliability)*
**Goal:** survive transient 429/500/503/529 + honor `Retry-After` instead of dying mid-run.

**Current state:** `chat.rs::check_status_code()` (lines 94-119) *detects* 429 but
returns immediately — **no retry anywhere**; no 529 case.

**Key files / approach:**
- `shai-llm/src/chat.rs` — wrap the `.send()` calls (non-stream ~133-137, stream ~171-175)
  in a `RetryPolicy` (exponential backoff + jitter; retry 429/500/503/529; parse
  `retry-after` header in the 429 branch at ~line 110). Covers Anthropic + Mistral (ChatClient).
- openai_dive providers: wrap the `client.chat().create()` call site per provider
  (e.g. `providers/openai_compatible.rs:47`) — or a shared retry helper.

**Verify:** unit test with a mock returning 429 then 200; assert single success after backoff.

### 1.3 rtk-style tool-output compaction  *(source: rtk)*
**Goal:** compress noisy tool output before it enters the trace (rtk claims 60-90% on dev commands).

**Single chokepoint (confirmed):** `shai-core/src/agent/actions/tools.rs:145-151` —
`spawn_tool_static` pushes `result.to_string()` into the trace. Insert compaction
between obtaining `result` (~143) and `result.to_string()` (~149). Tool name available via `call.tool_name`.

**Three tiers (conservative; always preserve errors, flag truncation):**
- **Tier A — generic budget guard (universal, biggest win):** strip ANSI, collapse
  consecutive duplicate lines (`…×N`), head+tail truncate to a token budget with an
  explicit `[… N lines omitted …]` marker. Applies to every `ToolResult`.
- **Tier B — bash command-aware:** match on the command (`git status`/`diff`,
  `cargo test`/`build`/`clippy`, `pytest`, `grep`, `ls`) and reformat (failures-only,
  grouped). Output formatting lives in `shai-core/src/tools/bash/bash.rs:204-222`.
- **Tier C — native tools:** `ls`/`find` already structured → emit compact grouped form.

**Config:** opt-out / budget knobs via the config system (see Phase 2 config notes).
**Risk:** over-compression → model re-runs commands (net loss). Mitigate: keep all
error/failure signal; never silent-truncate.

**Verify:** snapshot tests on representative noisy outputs (large `cargo build`,
multi-thousand-line log) asserting size reduction + preserved error lines.

---

## Phase 2 — Ecosystem & standards (interoperability)

### 2.1 AGENTS.md support + SHAI.md demotion + Memory Tier 1  *(source: AGENTS.md standard; merged)*
**Goal:** adopt the cross-tool `AGENTS.md` standard as the canonical context file,
keep `SHAI.md` as an optional shai-specific *override/supplement*, and make project
context **agent-writable** (Memory Tier 1).

**Current state:** `SHAI.md` is read once, relative to CWD, frozen in an `OnceLock`,
silent on failure — `shai-core/src/runners/coder/prompt.rs:150-151`. No write path.
No repo-root discovery (`runners/coder/env.rs:is_git_repo()` checks `.git` in CWD only,
doesn't walk up).

**Key changes:**
- New layered loader (replaces the `OnceLock` blob): discover **AGENTS.md** canonical
  (walk up to git root; optional global `~/.config/shai/AGENTS.md`), then append
  **SHAI.md** as override. Add a find-git-root helper (none exists; `walkdir` is already a dep).
  Inject via the existing placeholder pattern (`{{SHAI}}` → add `{{AGENTS}}`) in
  `render_system_prompt_template()` (`prompt.rs:84-182`).
- Fix the three bugs while here: drop `OnceLock` freeze (so writes take effect),
  resolve repo-root path (not CWD), surface read errors instead of `unwrap_or_default()`.
- **Memory Tier 1 (writable):** a `MemoryTool` implementing the `Tool` trait
  (`shai-core/src/tools/types.rs:118-145`), registered in
  `shai-core/src/agent/builder.rs:create_default_tools()` (lines 70-86), that appends
  curated facts to AGENTS.md (or a dedicated memory file). Optionally a `#`-style quick-add.
- Back-compat: still read `SHAI.md` if present; announce as legacy/override.

**Verify:** project with nested dirs + AGENTS.md at root + SHAI.md override → confirm
merged context in the system prompt; memory tool appends and is visible next turn (no restart).

### 2.2 Skills (SKILL.md) support  *(source: Skills convention)*
**Goal:** composable, **model-selected**, on-demand procedural capabilities with
progressive disclosure (token-efficient) — the one extensibility axis shai lacks
(it has MCP=tools, agent configs=personas, but no skills).

**Key approach (reuse existing machinery):**
- Discover `SKILL.md` folders in `~/.config/shai/skills/` + project `.shai/skills/`.
- Inject only each skill's **name + description** (a catalog) into the system prompt
  via the same placeholder mechanism — full body loads on demand.
- Loading on match: minimal version uses the existing `Read` tool to pull the matched
  `SKILL.md` body; bundled scripts run via the existing `bash` tool. A dedicated
  `skill` tool is a nicer affordance (optional).
- Position clearly vs MCP (tools) and agent configs (personas) to avoid redundancy.

**Risk:** discovery quality depends entirely on description quality (prompt/UX problem).
**Verify:** a sample skill triggers only on relevant requests; catalog token cost is bounded (metadata only).

### 2.3 Config import from .claude / .cursor  *(LIGHTWEIGHT NOTE — deferred)*
Depends on 2.1/2.2. Sketch only: an **opt-in** `shai import` command that detects and
converts `CLAUDE.md`/`.claude/`, `.cursorrules`, `.cursor/rules` into shai's
`AGENTS.md`/config so shai drops into existing repos with minimal user intervention.
Detailed design deferred until AGENTS.md + skills land. Not in active scope.

### 2.4 Memory Tier 2  *(second phase, deferred within ecosystem)*
Layered/merged context across global + project + nested, live reload, richer memory
store. Builds directly on 2.1. Retrieval/embeddings memory is explicitly out of scope (ceiling).

---

## Phase 3 — Editing precision & correctness  *(source: dirac + generic)*

### 3.1 Multi-file batch edits  *(dirac; lowest-risk editing win)*
**Goal:** apply edits across multiple files in one tool call (cross-file refactors =
1 roundtrip, not N).

**Current state:** `MultiEditTool` is **single-file**, sequential, atomic
(`shai-core/src/tools/fs/multiedit/multiedit.rs:26-61`; params
`multiedit/structs.rs` = one `file_path` + `Vec<EditOperation>`).
**Approach:** new tool/params accepting `Vec<(file_path, Vec<EditOperation>)>`, reusing
`EditTool::perform_edit_on_content` (`fs/edit/edit.rs:157`) per file; extend
`FsOperationLog` read-before-edit validation across the batch.

### 3.2 Hash-anchored edits  *(dirac; standout, higher effort)*
**Goal:** target edits by stable per-line hash instead of copying exact `old_string`,
fixing the known failure modes of string-match (non-unique match, whitespace
sensitivity) and cutting input tokens on the high-frequency edit path.

**Approach:** Read tool emits a short stable hash per line/region
(`fs/read/read.rs:format_lines` ~109-123, currently `{:4}: {}`); edits accept a
`@<hash>` anchor resolved in `perform_edit_on_content`. Must coexist with the existing
`old_string` path and handle drift (re-anchor between read and edit).

### 3.3 Post-edit verification feedback loop  *(generic; correctness)*
**Goal:** edit → run project check (`cargo check`/`tsc`/`ruff`) → feed diagnostics back → fix.
Currently the agent edits blind.
**Approach:** hook after `FsOperationLog::log_operation(Edit/MultiEdit)`
(`shai-core/src/tools/fs/operation_log.rs:40-58`); run a configured check command via
the bash tool; attach diagnostics to the result / emit an event
(extend `AgentEvent` family in `agent/events.rs`).

### 3.4 AST foundation  *(dirac; greenfield — note)*
**Reality check:** tree-sitter is a **phantom dependency** today — declared in
`shai-core/Cargo.toml:17-18` but **never called**, with **no grammar crates** in
Cargo.lock; the only "highlighter" (`tools/highlight.rs`) is dead, buggy
string-replace. So AST is **build-from-scratch**, not "leverage existing infra."
Foundation for symbol-map/outline Read (token-efficient large-file reads) and
structural edits. High effort; sequence after 3.1-3.3 or move to backlog.

---

## Backlog (next roadmap / study)

- **Multi-model (lead/worker) routing** *(goose)* — strong "lead" model plans, cheap
  "worker" model executes. Confirmed gap (shai is single-model). Major cost lever;
  `Brain`/`ToolCallMethod` already abstracted (`agent/agent.rs:65`). Strong candidate
  to promote once Phase 1 lands.
- **Prompt caching for openai_dive providers** — wrapper layer so caching isn't
  Anthropic-only (follow-up to 1.1).
- **Recipes + scheduler** *(goose)* — parameterized, shareable, schedulable headless
  workflows (distinct from skills). Fits shai's automation/headless niche.
- **Session persistence / resume in the CLI** *(NEEDS STUDY)* — exists only in HTTP mode
  (`shai-http/src/session/persist.rs`, env-gated). Study lifting it to the CLI
  (`--resume`/`--continue`).
- **Subagent context isolation** — a delegate tool (e.g. expose `searcher` as dispatchable)
  returning a digest to keep the main trace lean.
- **Bash hard sandbox** — Landlock/seccomp on Linux as defense-in-depth atop `ClaimManager`
  (bash currently runs unsandboxed: `tools/bash/bash.rs:137`).
- **Eval harness** *(goose)* — fixed tasks → outcome assertions, to measure whether these
  changes help. Pairs with CI hygiene fixes below.

### Hygiene (independent, low-effort; surfaced during review)
- CI only builds — add `cargo test` / `clippy -D warnings` / `fmt --check` (`.github/workflows/ci.yml`).
- Remove crate-wide `dead_code`/`unused_variables` lint suppression (masked the dead tree-sitter code).
- Remove the phantom tree-sitter deps + dead `tools/highlight.rs` *unless* 3.4 is scheduled.
- Replace fragile string-parsing in the `#[tool]` macro (`shai-macros/src/lib.rs:25-59`) with `syn`.

---

## Suggested sequencing

1. **Cross-cutting measurement** (gates Phase 1 claims).
2. **Phase 1**: 1.1 Anthropic caching → 1.2 retry/backoff → 1.3 output compaction.
3. **Phase 2**: 2.1 AGENTS.md + memory Tier 1 → 2.2 skills (2.3/2.4 deferred).
4. **Phase 3**: 3.1 multi-file → 3.3 feedback loop → 3.2 hash edits → 3.4 AST.
5. Promote from backlog as capacity allows (lead/worker routing is the strongest next cost lever).

## Overall verification
- Phase 1: multi-turn Anthropic session shows cache reads + measurable input-token drop;
  injected-429 test recovers; compaction snapshot tests show reduction with errors intact.
- Phase 2: nested-repo context merge correct; memory write visible same session; skill
  triggers only when relevant.
- Phase 3: cross-file refactor in one call; intentionally-broken edit surfaces diagnostics
  and self-corrects; hash anchor resolves after an intervening edit.
