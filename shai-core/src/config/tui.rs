use std::fmt;

use serde::{Deserialize, Serialize};

/// A keyboard key code. Mirrors `crossterm::event::KeyCode` subset relevant for config.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyCode {
    Char(char),
    Escape,
    Tab,
    Enter,
    Backspace,
    Delete,
    Left,
    Right,
    Up,
    Down,
    PageUp,
    PageDown,
    Home,
    End,
    Space,
}

/// Keyboard modifier flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyModifiers(u8);

impl KeyModifiers {
    pub const NONE: Self = Self(0);
    pub const SHIFT: Self = Self(1 << 0);
    pub const CONTROL: Self = Self(1 << 1);
    pub const ALT: Self = Self(1 << 2);
    pub const SUPER: Self = Self(1 << 3);

    pub fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    pub fn is_empty(self) -> bool {
        self.0 == 0
    }
}

impl std::ops::BitOr for KeyModifiers {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

/// A parsed key binding: a combination of modifiers + key code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyBinding {
    pub code: KeyCode,
    pub modifiers: KeyModifiers,
}

impl KeyBinding {
    pub fn new(code: KeyCode, modifiers: KeyModifiers) -> Self {
        Self { code, modifiers }
    }
}

impl fmt::Display for KeyBinding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut parts: Vec<&str> = Vec::new();
        if self.modifiers.contains(KeyModifiers::CONTROL) {
            parts.push("ctrl");
        }
        if self.modifiers.contains(KeyModifiers::SUPER) {
            parts.push("super");
        }
        if self.modifiers.contains(KeyModifiers::SHIFT) {
            parts.push("shift");
        }
        if self.modifiers.contains(KeyModifiers::ALT) {
            parts.push("alt");
        }
        let key_name = match self.code {
            KeyCode::Char(c) => return write!(f, "{}+{}", parts.join("+"), c),
            KeyCode::Escape => "esc",
            KeyCode::Tab => "tab",
            KeyCode::Enter => "enter",
            KeyCode::Backspace => "backspace",
            KeyCode::Delete => "delete",
            KeyCode::Left => "left",
            KeyCode::Right => "right",
            KeyCode::Up => "up",
            KeyCode::Down => "down",
            KeyCode::PageUp => "pageup",
            KeyCode::PageDown => "pagedown",
            KeyCode::Home => "home",
            KeyCode::End => "end",
            KeyCode::Space => "space",
        };
        parts.push(key_name);
        write!(f, "{}", parts.join("+"))
    }
}

impl Serialize for KeyBinding {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for KeyBinding {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        parse_binding(&s).map_err(serde::de::Error::custom)
    }
}

fn parse_binding(s: &str) -> Result<KeyBinding, String> {
    let s_lower = s.to_lowercase();
    let parts: Vec<&str> = s_lower.split('+').collect();
    if parts.is_empty() {
        return Err("empty key binding".to_string());
    }

    let mut modifiers = KeyModifiers::NONE;
    let mut key_part: Option<&str> = None;

    for part in &parts {
        match *part {
            "ctrl" | "control" => modifiers = modifiers | KeyModifiers::CONTROL,
            "cmd" | "super" => modifiers = modifiers | KeyModifiers::SUPER,
            "shift" => modifiers = modifiers | KeyModifiers::SHIFT,
            "alt" | "option" => modifiers = modifiers | KeyModifiers::ALT,
            "" => {}
            _ => {
                if key_part.is_some() {
                    return Err(format!("unexpected token '{}' in key binding '{}'", part, s));
                }
                key_part = Some(*part);
            }
        }
    }

    let key_str = key_part.ok_or_else(|| format!("missing key in binding '{}'", s))?;

    let code = match key_str {
        "esc" | "escape" => KeyCode::Escape,
        "tab" => KeyCode::Tab,
        "enter" | "return" => KeyCode::Enter,
        "backspace" | "bs" => KeyCode::Backspace,
        "delete" | "del" => KeyCode::Delete,
        "left" => KeyCode::Left,
        "right" => KeyCode::Right,
        "up" => KeyCode::Up,
        "down" => KeyCode::Down,
        "pageup" | "pgup" => KeyCode::PageUp,
        "pagedown" | "pgdn" => KeyCode::PageDown,
        "home" => KeyCode::Home,
        "end" => KeyCode::End,
        "space" | " " => KeyCode::Space,
        _ => {
            let chars: Vec<char> = key_str.chars().collect();
            if chars.len() == 1 {
                KeyCode::Char(chars[0])
            } else {
                return Err(format!("unknown key '{}'", key_str));
            }
        }
    };

    Ok(KeyBinding { code, modifiers })
}

/// TUI shortcut configuration. All fields default to standard bindings.
/// To add a new shortcut, add a line to the macro invocation below.
/// TUI shortcut configuration. All fields default to standard bindings.
/// To add a new shortcut, add a field with `#[serde(default = "fn_name")]`
/// and a matching entry in the `Default` impl below.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShortcutsConfig {
    #[serde(default = "default_ctrl_t")]
    pub toggle_theme: KeyBinding,
    #[serde(default = "default_ctrl_c")]
    pub exit: KeyBinding,
    #[serde(default = "default_esc")]
    pub cancel_task: KeyBinding,
    #[serde(default = "default_esc")]
    pub clear_input: KeyBinding,
    #[serde(default = "default_ctrl_v")]
    pub paste: KeyBinding,
    #[serde(default = "default_ctrl_l")]
    pub clear_screen: KeyBinding,
    #[serde(default = "default_ctrl_r")]
    pub regenerate: KeyBinding,
    #[serde(default = "default_ctrl_k")]
    pub copy_response: KeyBinding,
    #[serde(default = "default_alt_x")]
    pub expand_tool: KeyBinding,
    #[serde(default = "default_ctrl_o")]
    pub session_picker: KeyBinding,
    #[serde(default = "default_ctrl_p")]
    pub prompt_picker: KeyBinding,
    #[serde(default = "default_shift_tab")]
    pub cycle_agent_mode: KeyBinding,
}

impl Default for ShortcutsConfig {
    fn default() -> Self {
        Self {
            toggle_theme: default_ctrl_t(),
            exit: default_ctrl_c(),
            cancel_task: default_esc(),
            clear_input: default_esc(),
            paste: default_ctrl_v(),
            clear_screen: default_ctrl_l(),
            regenerate: default_ctrl_r(),
            copy_response: default_ctrl_k(),
            expand_tool: default_alt_x(),
            session_picker: default_ctrl_o(),
            prompt_picker: default_ctrl_p(),
            cycle_agent_mode: default_shift_tab(),
        }
    }
}

/// Top-level TUI configuration loaded from `tui.config.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TuiConfig {
    #[serde(default)]
    pub shortcuts: ShortcutsConfig,
}

impl Default for TuiConfig {
    fn default() -> Self {
        Self {
            shortcuts: ShortcutsConfig::default(),
        }
    }
}

impl TuiConfig {
    pub fn config_path() -> Result<std::path::PathBuf, Box<dyn std::error::Error>> {
        let config_dir = std::env::var("XDG_CONFIG_HOME")
            .map(std::path::PathBuf::from)
            .or_else(|_| {
                dirs::home_dir()
                    .map(|home| home.join(".config"))
                    .ok_or("Could not find home directory")
            })?;

        let shai_config_dir = config_dir.join("shai");
        std::fs::create_dir_all(&shai_config_dir)?;

        Ok(shai_config_dir.join("tui.config.json"))
    }

    pub fn load() -> Self {
        let Ok(path) = Self::config_path() else {
            return Self::from_env_or_default();
        };

        if !path.exists() {
            return Self::from_env_or_default();
        }

        let content = match std::fs::read(&path) {
            Ok(content) => content,
            Err(_) => return Self::from_env_or_default(),
        };

        let stripped = json_comments::StripComments::new(&content[..]);
        serde_json::from_reader(stripped).unwrap_or_default()
    }

    /// Build config from `SHAI_KEY_*` environment variables, falling back to defaults.
    fn from_env_or_default() -> Self {
        let mut shortcuts = ShortcutsConfig::default();
        if let Ok(val) = std::env::var("SHAI_KEY_TOGGLE_THEME") {
            if let Ok(kb) = parse_binding(&val) {
                shortcuts.toggle_theme = kb;
            }
        }
        if let Ok(val) = std::env::var("SHAI_KEY_EXIT") {
            if let Ok(kb) = parse_binding(&val) {
                shortcuts.exit = kb;
            }
        }
        if let Ok(val) = std::env::var("SHAI_KEY_CANCEL_TASK") {
            if let Ok(kb) = parse_binding(&val) {
                shortcuts.cancel_task = kb;
            }
        }
        if let Ok(val) = std::env::var("SHAI_KEY_CLEAR_INPUT") {
            if let Ok(kb) = parse_binding(&val) {
                shortcuts.clear_input = kb;
            }
        }
        if let Ok(val) = std::env::var("SHAI_KEY_PASTE") {
            if let Ok(kb) = parse_binding(&val) {
                shortcuts.paste = kb;
            }
        }
        Self { shortcuts }
    }
}

fn default_ctrl_t() -> KeyBinding {
    KeyBinding::new(KeyCode::Char('t'), KeyModifiers::CONTROL)
}
fn default_ctrl_c() -> KeyBinding {
    KeyBinding::new(KeyCode::Char('c'), KeyModifiers::CONTROL)
}
fn default_ctrl_v() -> KeyBinding {
    KeyBinding::new(KeyCode::Char('v'), KeyModifiers::CONTROL)
}
fn default_ctrl_l() -> KeyBinding {
    KeyBinding::new(KeyCode::Char('l'), KeyModifiers::CONTROL)
}
fn default_ctrl_r() -> KeyBinding {
    KeyBinding::new(KeyCode::Char('r'), KeyModifiers::CONTROL)
}
fn default_ctrl_k() -> KeyBinding {
    KeyBinding::new(KeyCode::Char('k'), KeyModifiers::CONTROL)
}
fn default_alt_x() -> KeyBinding {
    KeyBinding::new(KeyCode::Char('x'), KeyModifiers::ALT)
}
fn default_ctrl_o() -> KeyBinding {
    KeyBinding::new(KeyCode::Char('o'), KeyModifiers::CONTROL)
}
fn default_ctrl_p() -> KeyBinding {
    KeyBinding::new(KeyCode::Char('p'), KeyModifiers::CONTROL)
}
fn default_esc() -> KeyBinding {
    KeyBinding::new(KeyCode::Escape, KeyModifiers::NONE)
}
fn default_shift_tab() -> KeyBinding {
    KeyBinding::new(KeyCode::Tab, KeyModifiers::SHIFT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_key() {
        let kb = parse_binding("esc").unwrap();
        assert_eq!(kb.code, KeyCode::Escape);
        assert!(kb.modifiers.is_empty());
    }

    #[test]
    fn test_parse_ctrl_key() {
        let kb = parse_binding("ctrl+t").unwrap();
        assert_eq!(kb.code, KeyCode::Char('t'));
        assert!(kb.modifiers.contains(KeyModifiers::CONTROL));
        assert!(!kb.modifiers.contains(KeyModifiers::SHIFT));
    }

    #[test]
    fn test_parse_shift_tab() {
        let kb = parse_binding("shift+tab").unwrap();
        assert_eq!(kb.code, KeyCode::Tab);
        assert!(kb.modifiers.contains(KeyModifiers::SHIFT));
    }

    #[test]
    fn test_parse_ctrl_shift() {
        let kb = parse_binding("ctrl+shift+t").unwrap();
        assert_eq!(kb.code, KeyCode::Char('t'));
        assert!(kb.modifiers.contains(KeyModifiers::CONTROL));
        assert!(kb.modifiers.contains(KeyModifiers::SHIFT));
    }

    #[test]
    fn test_parse_super() {
        let kb = parse_binding("super+v").unwrap();
        assert_eq!(kb.code, KeyCode::Char('v'));
        assert!(kb.modifiers.contains(KeyModifiers::SUPER));
    }

    #[test]
    fn test_display_roundtrip() {
        let kb = parse_binding("ctrl+shift+t").unwrap();
        let s = kb.to_string();
        let kb2 = parse_binding(&s).unwrap();
        assert_eq!(kb, kb2);
    }

    #[test]
    fn test_default_config() {
        let config = ShortcutsConfig::default();
        assert_eq!(config.toggle_theme, KeyBinding::new(KeyCode::Char('t'), KeyModifiers::CONTROL));
        assert_eq!(config.exit, KeyBinding::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
        assert_eq!(config.cancel_task, KeyBinding::new(KeyCode::Escape, KeyModifiers::NONE));
        assert_eq!(config.cycle_agent_mode, KeyBinding::new(KeyCode::Tab, KeyModifiers::SHIFT));
    }

    #[test]
    fn test_serde_roundtrip() {
        let config = TuiConfig::default();
        let json = serde_json::to_string_pretty(&config).unwrap();
        let parsed: TuiConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(config.shortcuts.toggle_theme, parsed.shortcuts.toggle_theme);
        assert_eq!(config.shortcuts.exit, parsed.shortcuts.exit);
        assert_eq!(config.shortcuts.cycle_agent_mode, parsed.shortcuts.cycle_agent_mode);
    }
}
