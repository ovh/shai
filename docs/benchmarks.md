# Benchmarks

Shai includes a measurement harness that runs the agent against scripted tasks (bug fixes, features, refactors, search) and collects token usage, tool call counts, compaction ratios, and verification results.

## Running Benchmarks

```bash
# Run a single benchmark script
cargo run --example measurement_harness -- shai-core/examples/scripts/bugfix.json

# Run all scripts in a directory
cargo run --example benchmark_runner -- shai-core/examples/scripts/

# Run multiple scripts
cargo run --example benchmark_runner -- shai-core/examples/scripts/bugfix.json shai-core/examples/scripts/feature.json
```

The `benchmark_runner` compiles `measurement_harness` automatically, runs each script sequentially, and prints a summary table with task name, category, difficulty, status, token count, and duration.

## Benchmark Scripts

Each benchmark is defined as a JSON file:

```json
{
  "name": "task-name",
  "category": "bugfix",
  "difficulty": "medium",
  "goal": "Initial prompt describing the task for the agent",
  "prompts": [],
  "setup": "sh -c 'curl ... | tar xz && cd project'",
  "verify": [
    { "command": "cargo check", "expected_exit_code": 0 }
  ]
}
```

| Field | Description |
|-------|-------------|
| `name` | Display name for the task |
| `category` | Task category (`bugfix`, `feature`, `refactor`, `search`) |
| `difficulty` | Difficulty level (`easy`, `medium`, `hard`) |
| `goal` | The initial prompt sent to the agent |
| `prompts` | Optional additional turns (follow-up prompts) to send after the goal |
| `setup` | Optional shell command to prepare a fixture directory |
| `verify` | List of commands to run after the agent completes; all must exit 0 to pass |

## Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `HARNESS_TURN_TIMEOUT_MS` | `120000` | Maximum time (ms) per turn before timing out |

## Output

Each benchmark run creates a `benchmarks-<timestamp>/` directory containing:
- One subdirectory per task (named after the script file stem)
- `runtime.log` — detailed agent execution log
- `session.log` — summary of token usage, tool calls, and verification results
- `diff.patch` — git diff of changes made by the agent

The harness also prints a JSON report wrapped between `<<<HARNESS_JSON_BEGIN>>>` and `<<<HARNESS_JSON_END>>>` markers for programmatic consumption.
