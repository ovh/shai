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
        "OVH_API_KEY": "your-api-key"
      },
      "model": "qwen3-32b-instruct",
      "tool_method": "FunctionCall"
    }
  ],
  "selected_provider": 0,
  "mcp_configs": {},
  "roles": {
    "planner": "ovhcloud/another-model"
  }
}
```

### Fields

| Field | Type | Description |
|-------|------|-------------|
| `providers` | [ProviderConfig](#providerconfig)[] | List of configured LLM providers |
| `selected_provider` | integer | Index into `providers` array for the active provider |
| `mcp_configs` | object&lt;string, object&gt; | MCP server configurations |
| `roles` | object&lt;string, string&gt; | Optional. Named secondary models, see [Roles](#roles) |

### ProviderConfig

| Field | Type | Description |
|-------|------|-------------|
| `provider` | string | Provider identifier (e.g. `"ovhcloud"`, `"openai"`, `"anthropic"`) |
| `env_vars` | object&lt;string, string&gt; | Environment variables passed to the provider (API keys, base URLs) |
| `model` | string | Model identifier |
| `tool_method` | string | Tool call method (`"Auto"`, `"FunctionCall"`, `"FunctionCallRequired"`, `"StructuredOutput"`) |

## Roles

A session runs on one primary model, the one `selected_provider` points to. The optional `roles` section names additional models that opt-in features can use next to it. Nothing uses a role unless a feature asks for it by name, so a config without `roles` behaves exactly as before.

```jsonc
{
  "providers": [
    // an "openai" entry and an "ollama" entry, added with `shai auth`
  ],
  "selected_provider": 0,
  "roles": {
    "planner": "openai/gpt-4o-mini",
    "cheap": "ollama/qwen3:8b"
  }
}
```

Each role is a `<provider>/<model>` string:

- `<provider>` must be the `provider` of an entry in `providers`. The role reuses that entry's `env_vars` (API key, base URL) and only changes the model. This is how you point a role at a second model of a provider you already configured, without entering its credentials again.
- `<model>` is everything after the first `/`, so model names that contain slashes work (`openrouter/anthropic/claude-sonnet-4`). It is limited to visible ASCII characters.
- Role names use lowercase letters, digits, `-` and `_`.

The rules are strict on purpose, so that a mistake does not quietly send a conversation to a model you did not pick:

- A role that is not listed is simply off.
- Roles only come from your own config file. The starter config Shai downloads on first run never carries any.
- A role is a string. It cannot hold credentials of its own; an object is rejected.
- A role that names a provider with no entry in `providers`, or whose reference is malformed, is an error when a feature tries to use it. Shai does not substitute the primary model, another provider, or the built-in default configuration.
- A role only reads the provider entry it names. Unlike the primary model, it does not pick a missing key or base URL up from your shell environment. Concretely, an entry of a provider that needs an API key must hold its own to back a role (so an `ovhcloud` entry without `OVH_API_KEY`, which uses anonymous access, cannot), and an `ollama` entry must hold its `OLLAMA_BASE_URL`; its `OLLAMA_API_KEY` stays optional.
- The reference must single out one way to reach the model. If entries of that provider are configured for exactly that model, one of those is used: they must be usable and agree with each other. Otherwise every entry of that provider must be usable and hold the same settings. If an entry of the provider you name cannot back a role, or two of them point at different endpoints, the reference is rejected rather than routed to whichever entry happens to work. In practice: if the keyless starter `ovhcloud` entry is still next to your own, remove it with `shai auth`, or name the model your own entry is configured for.
- If the `roles` section is not an object, if one of its keys is not a valid role name or appears twice, or if the file has two `roles` sections, every role is rejected until that is fixed.

Two limits to keep in mind:

- Resolving a role is offline. Shai does not ask the provider whether it serves the model, so a wrong model name shows up as that provider's error the first time the role is used. A role also inherits the entry's base URL: if that URL serves a single model, as OVHcloud's per-model endpoints do, name that model in the role.
- A wrong value inside `roles`, or a `roles` section written twice, does not stop the rest of the file from loading. A JSON syntax error does, in `roles` like anywhere else in the file, and so does a `roles` section that is not valid UTF-8 text. The primary model then falls back to the built-in default, as it always has when the file cannot be read.

## Management

The auth configuration is managed via the `shai auth` interactive wizard:

```bash
shai auth
```

You can also run auth from within the TUI using the `/auth` slash command — this will save your current conversation context and restart the agent with the new provider settings.
