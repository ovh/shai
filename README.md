# SHAI

shai is a coding agent, your pair programming buddy that lives in the terminal. Written in rust with love <3

![Shai CLI Screenshot](./docs/assets/shai.png)

## Features

- **Interactive coding agent** — Chat with shai in your terminal to write code, fix bugs, and get answers
- **Headless mode** — Pipe prompts directly into shai for scripting and automation
- **HTTP server** — Run shai as a service with OpenAI-compatible APIs and SSE streaming
- **Shell assistant** — Automatically suggests fixes when commands fail in your terminal
- **Project context** — Load project-specific information via `AGENTS.md` files
- **MCP Support** — Configure specialized agents with MCP and OAuth support
- **Skills** — Extend shai with composable, on-demand procedural instructions
- **Multiple LLM providers** — Works with OVHCloud, OpenAI, Anthropic, Mistral, Ollama, OpenRouter

## Installation

### Latest stable release

```bash
curl -fsSL https://raw.githubusercontent.com/ovh/shai/main/install.sh | sh
```

### Nightly version

```bash
curl -fsSL https://raw.githubusercontent.com/ovh/shai/main/install.sh | SHAI_RELEASE=unstable sh
```

The `shai` binary will be installed in `$HOME/.local/bin`

## Quick Start

By default `shai` uses OVHCloud as an anonymous user meaning you will be rate limited! If you want to sign in with your account or select another provider, run:

```bash
shai auth
```

![shai auth](./docs/assets/auth.gif)

Once you have a provider set up, you can run shai:

```bash
shai
```

![shai](./docs/assets/shai-hello-world.gif)

## Usage

### Interactive Mode

Simply run `shai` to start the interactive coding agent:

```bash
shai                          # Start TUI
shai -a myagent              # Start TUI with a custom agent
shai session                 # Start TUI with session picker
shai session <id>            # Start TUI restoring a specific session
shai session latest          # Start TUI restoring the most recent session
shai -r <session-id>        # Restore a session by ID
shai --latest                # Restore the most recent session
```

### Headless Mode

Run shai without the TUI for scripting and automation:

```bash
shai -p "make me a hello world in main.py"
echo "make me a hello world in main.py" | shai
```

Chain multiple shai calls together:

```bash
echo "make me a hello world in main.py" | shai --trace | shai -p "now run it!"
```

### Interactive Pipe Mode

Pipe content into shai and continue the conversation in the TUI:

```bash
echo "analyze this code" | shai -i
```

### HTTP Server Mode

Run shai as an HTTP service with SSE streaming support:

```bash
shai serve --port 3000
```

![shai http](./docs/assets/shai-http.png)

Available API endpoints:

- **POST /v1/chat/completions** — OpenAI Chat Completions API (ephemeral mode)
- **POST /v1/responses** — OpenAI Responses API (stateful/stateless)
- **GET /v1/responses/{id}** — Get response by ID
- **POST /v1/responses/{id}/cancel** — Cancel a response
- **POST /v1/multimodal** — Simple multimodal API (streaming)
- **POST /v1/multimodal/{session_id}** — Simple multimodal API (with session)

### Shell Assistant

shai can also act as a shell assistant — when a command fails, it will propose a fix. This works by injecting command hooks while monitoring your terminal output.

```bash
shai on      # Start monitoring
shai off     # Stop monitoring
shai status   # Check if shai is active
```

![Shai Shell Assistant](./docs/assets/shai-shell.png)

## Configuration

### Project Context File

Shai supports the [AGENTS.md](https://agents.md/) convention — a standard markdown file at the root of your project containing build steps, code style guidelines, testing instructions, and other context for AI coding agents.

Shai loads context from two files (in priority order):

1. **`AGENTS.md`** — the canonical project context file (recommended).
2. **`SHAI.md`** — legacy override/supplement (deprecated, use `AGENTS.md` instead).

For nested projects in a monorepo, place an `AGENTS.md` in each subdirectory — the closest one to the edited file takes precedence.

### Custom Agents

Instead of a single global configuration, you can create custom agent configurations with specific providers, tools, and system prompts.

```bash
shai agent           # Open agent picker in TUI
shai agent myagent   # Launch TUI with a specific agent
shai list agent      # List available agents
```

Agent configs live in `~/.config/shai/agents/`. See [Agent Configuration](./docs/configurations/agents.md) for the full schema.

### Skills

Skills are composable, on-demand procedural instructions that extend shai's capabilities:

```
.shai/skills/
└── my-skill/
    └── SKILL.md
```

See [Skills documentation](./docs/skills.md) for details.

### TUI Slash Commands

| Command | Description |
|---------|-------------|
| `/auth` | Configure AI provider |
| `/agent` | Switch agent (keeps conversation context) |
| `/restore [index\|id]` | Restore a previous session |
| `/latest` | Restore the most recent session |
| `/temp <float>` | Set sampling temperature |
| `/tc <method>` | Set tool call method (`auto`, `fc`, `fc2`, `so`) |
| `/theme [dark\|light\|toggle]` | Set or toggle theme |
| `/tokens` | Display token usage |
| `/tools` | List all registered tools |
| `/skills` | List available skills |
| `/mcp` | List MCP servers and connection status |
| `/regenerate` | Regenerate the last response |
| `/exit` | Exit the TUI |

See [TUI Configuration](./docs/configurations/tui.md) for keyboard shortcuts and customization.

## Development

```bash
git clone git@github.com:ovh/shai.git
cd shai
cargo build --release
```

## Benchmarks

Shai includes a measurement harness that runs the agent against scripted tasks. See [Benchmarks documentation](./docs/benchmarks.md) for details.
