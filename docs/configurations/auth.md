# Auth Configuration

The auth configuration file stores provider credentials and LLM settings.

## Location

```
~/.config/shai/auth.config.json
```

Or via `XDG_CONFIG_HOME`:

```
$XDG_CONFIG_HOME/shai/auth.config.json
```

## Migration

Previous versions of Shai used `auth.config` (without `.json` extension). If `auth.config.json` does not exist, Shai will fall back to reading the legacy `auth.config` file automatically. No manual migration is needed.

## Schema

```jsonc
{
  "providers": [
    {
      "provider": "ovhcloud",
      "env_vars": {
        "OVHAI_API_KEY": "your-api-key"
      },
      "model": "qwen3-32b-instruct",
      "tool_method": "function_call"
    }
  ],
  "selected_provider": 0,
  "mcp_configs": {}
}
```

### Fields

| Field | Type | Description |
|-------|------|-------------|
| `providers` | [ProviderConfig](#providerconfig)[] | List of configured LLM providers |
| `selected_provider` | integer | Index into `providers` array for the active provider |
| `mcp_configs` | object&lt;string, object&gt; | MCP server configurations |

### ProviderConfig

| Field | Type | Description |
|-------|------|-------------|
| `provider` | string | Provider identifier (e.g. `"ovhcloud"`, `"openai"`, `"anthropic"`) |
| `env_vars` | object&lt;string, string&gt; | Environment variables passed to the provider (API keys, base URLs) |
| `model` | string | Model identifier |
| `tool_method` | string | Tool call method (`"auto"`, `"function_call"`, `"function_call_required"`, `"structured_output"`) |

## Management

The auth configuration is managed via the `shai auth` interactive wizard:

```bash
shai auth
```

You can also run auth from within the TUI using the `/auth` slash command — this will save your current conversation context and restart the agent with the new provider settings.
