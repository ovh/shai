# CLI Reference

Complete reference for all shai commands and flags.

## Global Flags

These flags work with most commands.

| Flag | Description |
|------|-------------|
| `-p`, `--prompt <text>` | Run in headless mode with the given prompt |
| `-a`, `--agent <name>` | Use a specific agent |
| `-i`, `--interactive` | Pipe input then show TUI with context |
| `-r`, `--restore <id>` | Restore a previous session by ID |
| `--latest` | Restore the most recent session |
| `--trace` | Dump entire trace upon completion (headless mode only) |
| `--temperature <float>` | Set the LLM sampling temperature |
| `--tools <list>` | Specify which tools to use (comma-separated) |
| `--remove <list>` | Remove specific tools from the default set |
| `--list-tools` | List all available tools |
| `-v`, `--version` | Show version information |

## Commands

### `shai`

Start the interactive TUI.

```bash
shai                        # Default TUI
shai -a myagent            # TUI with a custom agent
shai --latest              # TUI restoring the most recent session
shai -r <session-id>      # TUI restoring a specific session
```

### `shai auth`

Open the auth/provider configuration TUI. Lets you add, edit, and remove LLM providers.

```bash
shai auth
```

### `shai agent`

Launch the TUI with an agent picker. Optionally specify an agent name to skip the picker.

```bash
shai agent              # Open agent picker
shai agent myagent     # Launch TUI with a specific agent
shai agent list        # List available agents
```

### `shai session`

Launch the TUI with a session picker. Optionally specify a session ID or `latest` to skip the picker.

```bash
shai session            # Open session picker
shai session <id>       # Restore a specific session
shai session latest     # Restore the most recent session
```

### `shai list`

List agents, sessions, or skills.

```bash
shai list              # List everything
shai list agent        # List agents only
shai list session      # List sessions only
shai list skills       # List skills only
```

### `shai serve`

Start the HTTP server with OpenAI-compatible APIs and SSE streaming.

```bash
shai serve                              # Default: 127.0.0.1:3000
shai serve --port 8080                  # Custom port
shai serve --host 0.0.0.0 --port 8080  # Bind to all interfaces
shai serve --ephemeral                 # New agent per request
shai serve -a myagent                  # Use a specific agent
```

| Flag | Default | Description |
|------|---------|-------------|
| `--host` | `127.0.0.1` | Host to bind to |
| `-p`, `--port` | `3000` | Port to bind to |
| `--ephemeral` | `false` | Spawn new agent per request |
| `--max-sessions` | unlimited | Maximum number of concurrent sessions |
| `-a`, `--agent` | — | Agent name to serve |

### `shai on` / `shai off` / `shai status`

Manage the shell assistant PTY session.

```bash
shai on                 # Start monitoring shell
shai off                # Stop monitoring
shai status             # Check if shai is active
```

### `shai import`

Import configuration from `.claude` or `.cursor` into `AGENTS.md`.

```bash
shai import             # Append to existing AGENTS.md
shai import --overwrite # Overwrite existing AGENTS.md
```

## Headless Mode

Run shai without the TUI for scripting and automation:

```bash
# Explicit prompt
shai -p "make me a hello world in main.py"

# Piped input
echo "make me a hello world in main.py" | shai

# With agent
shai -a myagent -p "fix this bug"

# With temperature
shai -p "rewrite this function" --temperature 0.5

# Chain commands
echo "make me a hello world in main.py" | shai --trace | shai -p "now run it!"
```

## Interactive Pipe Mode

Pipe content into shai and continue the conversation in the TUI:

```bash
echo "analyze this code" | shai -i
cat error.log | shai -i
git diff | shai -i
```

The piped content is sent as the first user message, and the TUI opens immediately after.
