# Compaction & Agent Loop Improvements

## Overview

Following the analysis of a benchmark run where the shai-harness ballooned to 2.9M tokens across 112 tool calls, we identified and implemented four improvements to prevent runaway agent behavior and reduce context window consumption.

## What We Did

### 1. Max Tool Calls Per Turn

Added a configurable limit (`max_tool_calls_per_turn`, default: 100) that tracks tool calls since the last user message. When the limit is hit, the agent injects a wrap-up message into the trace and transitions back to `Running` state, prompting the LLM to summarize and respond without further tool calls.

**Files modified:**
- `shai-core/src/config/agent.rs` — Added `max_tool_calls_per_turn` to `CompactionConfig`
- `shai-core/src/agent/agent.rs` — Added `tool_call_count` field, reset on new user input
- `shai-core/src/agent/actions/brain.rs` — Limit check in `process_next_step`

### 2. Duplicate Command Detection

Added a rotating command cache (`max_cached_commands`, default: 50) that stores normalized bash commands and their compacted results. When the agent attempts torun the same command twice, the cached result is returned with a `[cached]` prefix, skipping execution entirely.

**Files modified:**
- `shai-core/src/agent/agent.rs` — Added `command_cache` field
- `shai-core/src/agent/actions/tools.rs` — Cache lookup and storage in `spawn_tool_static`

### 3. Session-Level Trace Compaction

Added a `compact_trace_if_needed` function that replaces older `ChatMessage::Tool` entries with `[compacted]` when the total trace size exceeds `max_trace_chars` (default: 50,000). The most recent 10 messages are always preserved. Compaction runs on a temporary clone of the trace before sending to the LLM — the original trace is never modified.

**Files added/modified:**
- `shai-core/src/runners/compacter/trace.rs` — **New file** with `compact_trace_if_needed`
- `shai-core/src/runners/compacter/mod.rs` — Re-exported new module
- `shai-core/src/agent/brain.rs` — Added `max_trace_chars` to `ThinkerContext`
- `shai-core/src/agent/actions/brain.rs` — Populated `max_trace_chars` in `spawn_next_step`
- `shai-core/src/runners/coder/coder.rs` — Called `compact_trace_if_needed` in `next_step`

### 4. Batch-Fix Prompting

Added instructions to the system prompt guiding the agent to fix all compilation/test errors in a single pass before re-running build commands.

**File modified:**
- `shai-core/src/runners/coder/prompt.rs` — Added "Batch Error Fixes" section to `CODER_GUIDELINE`

## Benchmark Results

All four benchmark tasks passed verification across 8 independent runs (32 total task executions). Model used: `galere/GLM-5.2-NVFP4`.

### Per-Task Comparison

| Task | Metric | Baseline | Median (8 runs) | Change |
|------|--------|----------|-----------------|--------|
| **bugfix** | Input tokens | 165,110 | 69,456 | -57.9% |
| | Output tokens | 2,538 | 1,406 | -44.6% |
| | Tool calls | 26 | 12 | -53.8% |
| | Duration | 105.8s | 101.1s | -4.4% |
| | Compaction | 38.2% | 1.9% | — |
| **feature** | Input tokens | 565,095 | 189,570 | -66.4% |
| | Output tokens | 6,846 | 4,260 | -37.8% |
| | Tool calls | 40 | 31 | -22.5% |
| | Duration | 619.1s | 31.0s | -95.0% |
| | Compaction | 14.1% | 15.2% | — |
| **refactor** | Input tokens | 29,105 | 109,986 | +277.9% |
| | Output tokens | 1,416 | 2,639 | +86.4% |
| | Tool calls | 6 | 14 | +133.3% |
| | Duration | 114.4s | 119.9s | +4.8% |
| | Compaction | 7.1% | 5.7% | — |
| **search** | Input tokens | 75,924 | 59,510 | -21.6% |
| | Output tokens | 1,675 | 1,547 | -7.6% |
| | Tool calls | 14 | 12 | -14.3% |
| | Duration | 11.7s | 9.2s | -21.4% |
| | Compaction | 0.1% | 0.0% | — |

### Aggregate Totals

| Metric | Baseline | Median (8 runs) | Change |
|--------|----------|-----------------|--------|
| Total input tokens | 835,234 | 502,162 | -39.9% |
| Total output tokens | 12,475 | 11,019 | -11.7% |
| Total tool calls | 86 | 71 | -17.4% |

### Run-to-Run Variance

Because the LLM provider does not guarantee deterministic output at `temperature=0`, there is significant run-to-run variance:

| Metric | Min | Median | Max |
|--------|-----|--------|-----|
| Total input tokens | 226,750 | 502,162 | 835,234 |
| Total output tokens | 5,291 | 11,019 | 17,367 |
| Total tool calls | 55 | 71 | 86 |

The compaction improvements show the most benefit on the `feature` task (median 15.2% reduction), where large file reads and command outputs dominate the context window.

## Configuration

All settings are configurable via the agent's `CompactionConfig`:

```json
{
  "compaction": {
    "enabled": true,
    "max_output_chars": 8000,
    "max_tool_calls_per_turn": 100,
    "max_cached_commands": 50,
    "max_trace_chars": 200000
  }
}
```

| Setting | Default | Description |
|---------|---------|-------------|
| `max_tool_calls_per_turn` | `100` | Maximum tool calls per user turn before forcing a wrap-up |
| `max_cached_commands` | `50` | Number of recent bash commands to cache for duplicate detection |
| `max_trace_chars` | `200000` | Character threshold above which older tool results are compacted |
