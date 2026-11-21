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
  },
  "prewalk": {
    "into": "planner"
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
| `prewalk` | object | Optional. One-shot handoff to another model, see [Prewalk](#prewalk) |

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
- A wrong value inside `roles`, or a `roles` section written twice, does not stop the rest of the file from loading. A JSON syntax error does, in `roles` like anywhere else in the file, and so does a `roles` section that is not valid UTF-8 text. When the file cannot be read, the TUI and headless mode do not start a session (see [Prewalk](#prewalk)); `shai serve` and the shell assistant fall back to the built-in default, as they always have.

## Prewalk

Prewalk hands a session off to another model, once, while it is running. The session starts on the primary model. When the agent has a todo list **and** its first `write` or `edit` has succeeded, the next request goes to the target model instead, and so does every request after it. The usual use is to plan on a strong model and do the edits on a cheaper or local one.

It is off by default. Turn it on for one run with a flag, or for every session in the config:

```bash
shai --prewalk-into cheap                      # a role from "roles"
shai -p "fix the flaky test" --prewalk-into ollama/qwen3:8b
```

```jsonc
"prewalk": { "into": "cheap" }
```

The target is a role name or a `<provider>/<model>` reference and follows the same rules as [roles](#roles): it must resolve, offline, to a provider entry that holds its own credentials. The flag wins over the config.

What to expect:

- **A notice when it is armed and when it switches.** The TUI shows both in the conversation and updates the model in the status bar; headless mode prints one line on stderr for each.
- **Everything carries over.** The conversation so far, the todo list, the working directory and permissions are unchanged. Only the model that receives the next request changes. This also means the whole conversation so far, including the system prompt and everything tools returned, is sent to the target's provider.
- **It happens between two steps**, after all tool results of the step that contained the edit are in, never in the middle of a step. A failed or denied edit does not count. Without a todo list the session stays where it is until one exists.
- **Once.** After the switch the conversation stays on the target; nothing switches it back. In the TUI, an agent you start afterwards (another agent, a restored conversation, `/auth`) is a new agent on its own model, and prewalk does not move it: Shai says so with a `prewalk is over` line.
- **If the target cannot be resolved, the session does not start.** A role that is not configured, a provider with no entry, a `prewalk` section that is invalid or written twice: each is reported as an error and Shai exits with status 2 instead of running without the handoff. The error names a target taken from the config by its key (`prewalk.into`) and does not repeat its value, unless the value is the name of a role or provider that is configured. `--prewalk-into` on a command that starts no session (`shai serve`, `shai auth`, `shai --list-tools`, ...) is an error too. HTTP server mode does not use prewalk.
- **A config file that cannot be read stops the session as well.** If `auth.config.json` (or the legacy `auth.config`) exists and does not load, Shai cannot tell whether it asks for a handoff, so the TUI and headless mode exit with status 2 and say where the file is broken, without repeating what it holds, instead of running on the built-in default model as they did before. An agent the TUI starts later in the session (agent picker, restored session, `/auth`) follows the same rule: it is not started on the built-in default. No starter config is downloaded over such a file. `shai auth` and the other commands that start no session still work, and so does a first run with no config file at all.
- **An agent that could never trigger it is not armed.** The handoff needs the built-in `todo_write` tool and at least one of `write` and `edit`. If the agent's tools leave them out (`--tools`, `--remove`, a custom agent), Shai says `prewalk not armed` with the reason, in the same place as the armed notice, and the session runs on its own model. Tools of the same names served by an MCP server do not count.
- **The configuration is followed.** Each time the TUI starts an agent (after `/auth`, when you pick another agent, when you restore a conversation), a handoff that is still to come is resolved again against the configuration as it is then. If it no longer resolves, the handoff is given up with a `prewalk disarmed` line; it is never sent to credentials or an endpoint you have just removed.
- If the target's provider rejects the request after the switch (for example because the model name is wrong, or it does not accept the conversation another provider produced), that error is shown and the session stays on the target.

## Management

The auth configuration is managed via the `shai auth` interactive wizard:

```bash
shai auth
```

You can also run auth from within the TUI using the `/auth` slash command — this will save your current conversation context and restart the agent with the new provider settings.
