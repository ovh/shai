# TUI Configuration

The TUI configuration file stores customizable keyboard shortcuts and appearance settings.

## Location

```
~/.config/shai/tui.config.json
```

Or via `XDG_CONFIG_HOME`:

```
$XDG_CONFIG_HOME/shai/tui.config.json
```

If the file does not exist, default values are used.

## Schema

```jsonc
{
  "shortcuts": {
    "toggle_theme": "ctrl+t",
    "exit": "ctrl+c",
    "cancel_task": "esc",
    "clear_input": "esc",
    "paste": "ctrl+v",
    "clear_screen": "ctrl+l",
    "regenerate": "ctrl+r",
    "copy_response": "ctrl+k",
    "expand_tool": "ctrl+x",
    "session_picker": "ctrl+o",
    "prompt_picker": "ctrl+p",
    "cycle_agent_mode": "shift+tab"
  }
}
```

### Shortcut Reference

| Field | Default | Description |
|-------|---------|-------------|
| `toggle_theme` | `ctrl+t` | Toggle dark/light theme |
| `exit` | `ctrl+c` | Exit the TUI |
| `cancel_task` | `esc` | Cancel running agent task |
| `clear_input` | `esc` | Clear input buffer (double-tap within 1 second) |
| `paste` | `ctrl+v` | Paste from clipboard |
| `clear_screen` | `ctrl+l` | Clear screen and reset viewport |
| `regenerate` | `ctrl+r` | Regenerate last response |
| `copy_response` | `ctrl+k` | Copy last assistant response to clipboard |
| `expand_tool` | `ctrl+x` | Expand last tool result in alternate screen viewer |
| `session_picker` | `ctrl+o` | Open session picker |
| `prompt_picker` | `ctrl+p` | Open system prompt picker |
| `cycle_agent_mode` | `shift+tab` | Cycle agent mode (Plan/Manual/Auto) |

## Key Binding Format

Key bindings are specified as strings with modifiers separated by `+`:

| Modifier | Token |
|---------|-------|
| Control | `ctrl` |
| Super (Cmd) | `super` |
| Shift | `shift` |
| Alt (Option) | `alt` |

### Supported Keys

- Single characters: `a`-`z`, `0`-`9`
- Special keys: `esc`, `tab`, `enter`, `backspace`, `delete`, `space`
- Arrow keys: `up`, `down`, `left`, `right`
- Navigation: `pageup`, `pagedown`, `home`, `end`

### Examples

```
"ctrl+t"        // Ctrl+T
"shift+tab"     // Shift+Tab
"super+v"       // Cmd+V (macOS)
"ctrl+shift+p"  // Ctrl+Shift+P
"esc"            // Escape key
```

## Commands

The following slash commands are available in the TUI:

| Command | Description |
|---------|-------------|
| `/auth` | Configure AI provider (saves and restores conversation context) |
| `/agent` | Switch agent (keeps conversation context) |
| `/tc <method>` | Set tool call method: `auto`, `fc`, `fc2`, `so` |
| `/temp <float>` | Set sampling temperature |
| `/tokens` | Display token usage |
| `/theme [dark\|light\|toggle]` | Set or toggle theme |
| `/restore [index\|id]` | Restore a previous session |
| `/latest` | Restore the most recent session |
| `/skills` | List available skills |
| `/regenerate` | Regenerate the last response |
| `/tools` | List all registered tools |
| `/mcp` | List MCP servers and connection status |
| `/exit` | Exit the TUI |

## Migration

### From environment variables

Previous versions of Shai supported `SHAI_KEY_*` environment variables for shortcut customization. These are now deprecated. Env vars are **only** read when `tui.config.json` does not exist — if the config file is present, env vars are ignored entirely.

To migrate, create a `tui.config.json` file with your custom bindings.

| Env var | Shortcut |
|---------|----------|
| `SHAI_KEY_TOGGLE_THEME` | `toggle_theme` |
| `SHAI_KEY_EXIT` | `exit` |
| `SHAI_KEY_CANCEL_TASK` | `cancel_task` |
| `SHAI_KEY_CLEAR_INPUT` | `clear_input` |
| `SHAI_KEY_PASTE` | `paste` |

### From older versions

No migration is needed — the `tui.config.json` file is optional and defaults are used when absent.
