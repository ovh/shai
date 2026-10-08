use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use shai_core::config::tui::{KeyBinding, KeyCode as ConfigCode, KeyModifiers as ConfigKeyMods, TuiConfig};

/// Wrapper around `TuiConfig` that provides convenient key-matching against `crossterm::event::KeyEvent`.
pub struct Shortcuts {
    config: TuiConfig,
}

macro_rules! impl_shortcut_accessors {
    ($($name:ident),+) => {
        $(
        pub fn $name(&self) -> &KeyBinding {
            &self.config.shortcuts.$name
        }
        )+
    };
}

impl Shortcuts {
    pub fn load() -> Self {
        let config = TuiConfig::load();
        Self { config }
    }

    impl_shortcut_accessors!(
        toggle_theme, exit, cancel_task, clear_input, paste, clear_screen, regenerate,
        copy_response, expand_tool, session_picker, prompt_picker, cycle_agent_mode
    );

    pub fn matches(&self, key_event: &KeyEvent, binding: &KeyBinding) -> bool {
        key_event_to_binding(key_event) == *binding
    }
}

pub fn key_event_to_binding(event: &KeyEvent) -> KeyBinding {
    let code = match event.code {
        KeyCode::Char(c) => ConfigCode::Char(c),
        KeyCode::Esc => ConfigCode::Escape,
        KeyCode::Tab => ConfigCode::Tab,
        KeyCode::BackTab => ConfigCode::Tab,
        KeyCode::Enter => ConfigCode::Enter,
        KeyCode::Backspace => ConfigCode::Backspace,
        KeyCode::Delete => ConfigCode::Delete,
        KeyCode::Left => ConfigCode::Left,
        KeyCode::Right => ConfigCode::Right,
        KeyCode::Up => ConfigCode::Up,
        KeyCode::Down => ConfigCode::Down,
        KeyCode::PageUp => ConfigCode::PageUp,
        KeyCode::PageDown => ConfigCode::PageDown,
        KeyCode::Home => ConfigCode::Home,
        KeyCode::End => ConfigCode::End,
        _ => ConfigCode::Space,
    };

    let mut modifiers = ConfigKeyMods::NONE;
    if event.modifiers.contains(KeyModifiers::SHIFT) {
        modifiers = modifiers | ConfigKeyMods::SHIFT;
    }
    if event.modifiers.contains(KeyModifiers::CONTROL) {
        modifiers = modifiers | ConfigKeyMods::CONTROL;
    }
    if event.modifiers.contains(KeyModifiers::ALT) {
        modifiers = modifiers | ConfigKeyMods::ALT;
    }
    if event.modifiers.contains(KeyModifiers::SUPER) {
        modifiers = modifiers | ConfigKeyMods::SUPER;
    }

    KeyBinding::new(code, modifiers)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_defaults() {
        let shortcuts = Shortcuts::load();
        assert_eq!(shortcuts.toggle_theme(), &KeyBinding::new(ConfigCode::Char('t'), ConfigKeyMods::CONTROL));
        assert_eq!(shortcuts.exit(), &KeyBinding::new(ConfigCode::Char('c'), ConfigKeyMods::CONTROL));
    }

    #[test]
    fn test_match_ctrl_t() {
        let shortcuts = Shortcuts::load();
        let event = KeyEvent::new(
            KeyCode::Char('t'),
            KeyModifiers::CONTROL,
        );
        assert!(shortcuts.matches(&event, shortcuts.toggle_theme()));
    }

    #[test]
    fn test_match_ctrl_l() {
        let shortcuts = Shortcuts::load();
        let event = KeyEvent::new(KeyCode::Char('l'), KeyModifiers::CONTROL);
        assert!(shortcuts.matches(&event, shortcuts.clear_screen()));
    }

    #[test]
    fn test_match_shift_tab() {
        let shortcuts = Shortcuts::load();
        let event = KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT);
        assert!(shortcuts.matches(&event, shortcuts.cycle_agent_mode()));
    }

    #[test]
    fn test_no_match_wrong_modifier() {
        let shortcuts = Shortcuts::load();
        let event = KeyEvent::new(KeyCode::Char('t'), KeyModifiers::NONE);
        assert!(!shortcuts.matches(&event, shortcuts.toggle_theme()));
    }
}
