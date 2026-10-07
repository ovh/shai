use rand::Rng;
use ratatui::style::Color;

pub fn shai_logo() -> String {
    format!(
        r#"
  ███╗      ███████╗██╗  ██╗ █████╗ ██╗
  ╚═███╗    ██╔════╝██║  ██║██╔══██╗██║
     ╚═███  ███████╗███████║███████║██║
    ███╔═╝  ╚════██║██╔══██║██╔══██║██║
  ███╔═╝    ███████║██║  ██║██║  ██║██║
  ╚══╝      ╚══════╝╚═╝  ╚═╝╚═╝  ╚═╝╚═╝
                         version: {}
"#,
        env!("CARGO_PKG_VERSION")
    )
}

pub static SHAI_YELLOW: (u8, u8, u8) = (249, 188, 81);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Theme {
    Dark,
    Light,
}

#[derive(Debug, Clone, Copy)]
pub struct ThemePalette {
    pub input_text: Color,
    pub placeholder: Color,
    pub border: Color,
    pub status: Color,
    #[allow(dead_code)] // TODO: reserved for future method label rendering
    pub method_label: Color,
    pub suggestion_normal: Color,
    pub suggestion_selected_fg: Color,
    pub suggestion_selected_bg: Color,
    pub cursor_fg: Color,
    pub cursor_bg: Color,
    #[allow(dead_code)] // TODO: reserved for future error rendering
    pub error: Color,
    pub background: Color,
    #[allow(dead_code)] // TODO: reserved for future diff rendering
    pub diff_added: Color,
    #[allow(dead_code)] // TODO: reserved for future diff rendering
    pub diff_removed: Color,
    /// Status bar chip: provider
    pub chip_primary_fg: Color,
    pub chip_primary_bg: Color,
    /// Status bar chips: model, git branch, tokens
    pub chip_secondary_fg: Color,
    pub chip_secondary_bg: Color,
    /// Status bar chip: agent mode
    pub chip_accent_fg: Color,
    pub chip_accent_bg: Color,
    /// Status bar chips: location, tool-call method, notifications
    pub chip_warn_fg: Color,
    pub chip_warn_bg: Color,
}

impl Theme {
    /// Load the initial theme from `tui.config.json` (`theme` field).
    /// The `SHAI_TUI_THEME` env var is only honored when no config file exists
    /// (see `TuiConfig::load`), defaults to Dark.
    pub fn from_config() -> Self {
        use shai_core::config::tui::ThemePreference;
        match shai_core::config::tui::TuiConfig::load().theme {
            ThemePreference::Dark => Theme::Dark,
            ThemePreference::Light => Theme::Light,
        }
    }

    pub fn toggle(&mut self) {
        *self = match self {
            Theme::Dark => Theme::Light,
            Theme::Light => Theme::Dark,
        };
    }

    pub fn palette(&self) -> ThemePalette {
        match self {
            Theme::Dark => ThemePalette {
                input_text: Color::White,
                placeholder: Color::DarkGray,
                border: Color::DarkGray,
                status: Color::Yellow,
                method_label: Color::DarkGray,
                suggestion_normal: Color::White,
                suggestion_selected_fg: Color::Yellow,
                suggestion_selected_bg: Color::DarkGray,
                cursor_fg: Color::White,
                cursor_bg: Color::White,
                error: Color::Rgb(255, 100, 100),
                // Opaque so text contrast holds regardless of the terminal's own background
                background: Color::Black,
                diff_added: Color::Rgb(100, 255, 100),
                diff_removed: Color::Rgb(255, 100, 100),
                chip_primary_fg: Color::Black,
                chip_primary_bg: Color::Cyan,
                chip_secondary_fg: Color::White,
                chip_secondary_bg: Color::DarkGray,
                chip_accent_fg: Color::Black,
                chip_accent_bg: Color::Green,
                chip_warn_fg: Color::Black,
                chip_warn_bg: Color::Yellow,
            },
            Theme::Light => ThemePalette {
                input_text: Color::Black,
                placeholder: Color::Rgb(120, 120, 120),
                border: Color::Rgb(100, 100, 100),
                status: Color::Rgb(200, 100, 0),
                method_label: Color::Rgb(100, 100, 100),
                suggestion_normal: Color::Black,
                suggestion_selected_fg: Color::Black,
                suggestion_selected_bg: Color::Rgb(255, 220, 100),
                cursor_fg: Color::Black,
                cursor_bg: Color::Black,
                error: Color::Rgb(200, 0, 0),
                background: Color::White,
                diff_added: Color::Rgb(0, 150, 0),
                diff_removed: Color::Rgb(200, 0, 0),
                chip_primary_fg: Color::Black,
                chip_primary_bg: Color::Rgb(150, 220, 220),
                chip_secondary_fg: Color::Black,
                chip_secondary_bg: Color::Rgb(210, 210, 210),
                chip_accent_fg: Color::Black,
                chip_accent_bg: Color::Rgb(150, 220, 150),
                chip_warn_fg: Color::Black,
                chip_warn_bg: Color::Rgb(255, 220, 100),
            },
        }
    }
}

fn rgb_to_256_color(r: u8, g: u8, b: u8) -> u8 {
    let r_index = (r as f32 / 255.0 * 5.0).round() as u8;
    let g_index = (g as f32 / 255.0 * 5.0).round() as u8;
    let b_index = (b as f32 / 255.0 * 5.0).round() as u8;
    16 + (36 * r_index) + (6 * g_index) + b_index
}

pub fn apply_gradient(text: &str, from_color: (u8, u8, u8), to_color: (u8, u8, u8)) -> String {
    let lines: Vec<&str> = text.lines().collect();
    if lines.is_empty() {
        return String::new();
    }

    let max_width = lines
        .iter()
        .map(|line| line.chars().count())
        .max()
        .unwrap_or(0);
    if max_width == 0 {
        return String::new();
    }

    let mut result = String::new();

    for line in lines {
        let chars: Vec<char> = line.chars().collect();
        for (col, &ch) in chars.iter().enumerate() {
            if ch.is_whitespace() {
                result.push(ch);
            } else {
                let position = if max_width <= 1 {
                    0.0
                } else {
                    col as f32 / (max_width - 1) as f32
                };
                let r = (from_color.0 as f32 + (to_color.0 as f32 - from_color.0 as f32) * position)
                    as u8;
                let g = (from_color.1 as f32 + (to_color.1 as f32 - from_color.1 as f32) * position)
                    as u8;
                let b = (from_color.2 as f32 + (to_color.2 as f32 - from_color.2 as f32) * position)
                    as u8;
                let color_256 = rgb_to_256_color(r, g, b);
                result.push_str(&format!("\x1b[38;5;{}m{}\x1b[0m", color_256, ch));
            }
        }
        result.push('\n');
    }

    result
}

pub fn logo_cyan() -> String {
    let logo = shai_logo().replace("\n", "\r\n");
    apply_gradient(&logo, (255, 0, 255), (0, 255, 255))
}
/// Welcome block rendered inside the TUI at startup: logo + usage hints.
pub fn welcome_text() -> String {
    let logo = apply_gradient(shai_logo().trim_start(), SHAI_YELLOW, SHAI_YELLOW);
    // Explicit neutral grey: stays readable on both dark and light backgrounds
    let hints =
        "\x1b[38;5;244m? help \u{00B7} / commands \u{00B7} esc cancel \u{00B7} ctrl+c quit\x1b[0m";
    format!("{}\n\n{}", logo.trim_end(), hints)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Strip ANSI escape sequences so per-char gradient codes don't hide content
    fn strip_ansi(s: &str) -> String {
        let mut out = String::with_capacity(s.len());
        let mut chars = s.chars().peekable();
        while let Some(ch) = chars.next() {
            if ch == '\x1b' && matches!(chars.peek(), Some('[')) {
                chars.next();
                for c in chars.by_ref() {
                    if c.is_ascii_alphabetic() {
                        break;
                    }
                }
            } else {
                out.push(ch);
            }
        }
        out
    }

    #[test]
    fn test_welcome_text_contains_version_and_hints() {
        let text = strip_ansi(&welcome_text());
        assert!(text.contains(env!("CARGO_PKG_VERSION")));
        assert!(text.contains("? help"));
        assert!(text.contains("/ commands"));
    }

    #[test]
    fn test_both_palettes_define_chip_colors() {
        for theme in [Theme::Dark, Theme::Light] {
            let palette = theme.palette();
            assert_ne!(palette.chip_primary_fg, palette.chip_primary_bg);
            assert_ne!(palette.chip_secondary_fg, palette.chip_secondary_bg);
            assert_ne!(palette.chip_accent_fg, palette.chip_accent_bg);
            assert_ne!(palette.chip_warn_fg, palette.chip_warn_bg);
        }
    }
}
