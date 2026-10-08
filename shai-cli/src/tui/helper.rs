use ansi_to_tui::IntoText;
use ratatui::{
    layout::Rect,
    style::{Color, Style, Stylize},
    symbols::border,
    text::{Line, Span},
    widgets::{Block, Borders, Padding, Widget},
    Frame,
};

use super::command::CommandRegistry;
use shai_core::config::tui::{KeyBinding, TuiConfig};

pub struct HelpArea;

fn format_binding(binding: &KeyBinding) -> String {
    binding.to_string()
}

impl HelpArea {
    fn helper_msg(&self) -> String {
        let mut lines: Vec<String> = vec![
            "  ? to print help      tap esc twice to clear input".to_string(),
            "  / for commands       tap esc while agent is running to cancel".to_string(),
            "                       ctrl^c to exit".to_string(),
            String::new(),
            "  Available Commands:".to_string(),
        ];

        for cmd in CommandRegistry::commands().iter() {
            let args_suffix = if cmd.args.is_empty() {
                String::new()
            } else {
                format!(" <{}>", cmd.args.join("> <"))
            };
            lines.push(format!("  {:<14} {}", format!("{}{}", cmd.name, args_suffix), cmd.description));
        }

        lines.push(String::new());
        lines.push("  Shortcuts:".to_string());

        let config = TuiConfig::default();
        let s = &config.shortcuts;
        let bindings: [(&str, &str); 10] = [
            ("toggle_theme", "toggle dark/light theme"),
            ("clear_screen", "clear screen / reset viewport"),
            ("regenerate", "retry/regenerate last response"),
            ("copy_response", "copy last assistant response to clipboard"),
            ("paste", "paste from clipboard"),
            ("expand_tool", "expand tool output in full screen"),
            ("session_picker", "open session picker"),
            ("prompt_picker", "open system prompt picker"),
            ("cycle_agent_mode", "cycle agent mode (Plan/Manual/Auto)"),
            ("alt_r", "raw output for terminal selection"),
        ];

        let get_binding = |field: &str| -> Option<&KeyBinding> {
            match field {
                "toggle_theme" => Some(&s.toggle_theme),
                "clear_screen" => Some(&s.clear_screen),
                "regenerate" => Some(&s.regenerate),
                "copy_response" => Some(&s.copy_response),
                "paste" => Some(&s.paste),
                "expand_tool" => Some(&s.expand_tool),
                "session_picker" => Some(&s.session_picker),
                "prompt_picker" => Some(&s.prompt_picker),
                "cycle_agent_mode" => Some(&s.cycle_agent_mode),
                _ => None,
            }
        };

        for (field, desc) in bindings.iter() {
            let binding_str = match get_binding(field) {
                Some(b) => format_binding(b),
                None => "Alt+r".to_string(),
            };
            lines.push(format!("  {:<20} {}", binding_str, desc));
        }

        lines.join("\n")
    }
}

impl HelpArea {
    pub fn height(&self) -> u16 {
        self.helper_msg().lines().count() as u16
    }

    pub fn draw(&self, f: &mut Frame, area: Rect) {
        let helper_text = self.helper_msg();
        let x = helper_text.into_text().unwrap();
        let x = x.style(Style::default().fg(Color::White));
        f.render_widget(x, area);
    }
}
