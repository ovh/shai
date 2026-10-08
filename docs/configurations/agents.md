# Agent Configuration

Agent configuration files define per-agent settings including LLM provider, tools, system prompt, and more.

## Location

```
~/.config/shai/agents/{name}.config.json
```

Or via `XDG_CONFIG_HOME`:

```
$XDG_CONFIG_HOME/shai/agents/{name}.config.json
```

## Migration

Previous versions of Shai used `{name}.config` (without `.json` extension). If `{name}.config.json` does not exist, Shai will fall back to reading the legacy `{name}.config` file automatically. No manual migration is needed.

## Schema

```jsonc
{
  "name": "my-agent",
  "description": "A custom agent",
  "llm_provider": {
    "provider": "ovhcloud",
    "env_vars": {
      "OVHAI_API_KEY": "your-api-key"
    },
    "model": "qwen3-32b-instruct",
    "tool_method": "function_call"
  },
  "tools": {
    "builtin": ["read", "write", "bash"],
    "builtin_excluded": [],
    "mcp": {}
  },
  "system_prompt": "You are a helpful assistant.",
  "max_tokens": 4096,
  "temperature": 0.7,
  "compaction": {
    "enabled": true,
    "max_output_chars": 50000,
    "max_tool_calls_per_turn": 50,
    "max_cached_commands": 20,
    "max_trace_chars": 100000,
    "max_cached_reads": 20,
    "find_exclude_pattern": []
  },
  "verification": {
    "enabled": false,
    "timeout_secs": 30,
    "commands": {}
  }
}
```

### Fields

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `name` | string | — | Agent name |
| `description` | string | `""` | Human-readable description |
| `llm_provider` | object | — | LLM provider configuration (see [below](#agentproviderconfig)) |
| `tools` | object | — | Tool configuration (see [below](#agenttools)) |
| `system_prompt` | string | `""` | System prompt for the agent |
| `max_tokens` | integer | `4096` | Maximum tokens in response |
| `temperature` | number | `0.7` | Sampling temperature |
| `compaction` | object | — | Context compaction settings |
| `verification` | object | — | Tool verification settings |

### AgentProviderConfig

| Field | Type | Description |
|-------|------|-------------|
| `provider` | string | Provider identifier |
| `env_vars` | object&lt;string, string&gt; | Environment variables for the provider |
| `model` | string | Model identifier |
| `tool_method` | string | Tool call method |

### AgentTools

| Field | Type | Description |
|-------|------|-------------|
| `builtin` | string[] | List of built-in tools to enable |
| `builtin_excluded` | string[] | List of built-in tools to exclude |
| `mcp` | object&lt;string, object&gt; | MCP server tool configurations |

## Management

Agents are managed via the CLI:

```bash
shai agent              # Open agent picker in TUI
shai agent myagent     # Launch TUI with a specific agent
shai agent list         # List available agents
shai list agent         # Same as above (canonical form)
```

You can also run an agent in headless mode:

```bash
shai -a myagent -p "your prompt here"
```

Or switch agents from within the TUI using the `/agent` slash command (keeps conversation context).
