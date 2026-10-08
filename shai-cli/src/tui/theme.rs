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
}

impl Theme {
    /// Detect theme from environment variables and terminal capabilities
    /// Checks SHAI_TUI_THEME first, then COLORFGS/NO_COLOR, defaults to Dark
    pub fn from_env() -> Self {
        // Explicit override takes priority
        if let Ok(theme) = std::env::var("SHAI_TUI_THEME") {
            match theme.to_lowercase().as_str() {
                "light" => return Theme::Light,
                "dark" => return Theme::Dark,
                _ => {}
            }
        }

        // Respect NO_COLOR convention (https://no-color.org/)
        if std::env::var("NO_COLOR").is_ok() {
            // NO_COLOR doesn't necessarily mean light theme, but we can't detect
            // terminal background reliably, so fall through to default
        }

        // Default to Dark theme
        Theme::Dark
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
                background: Color::Black,
                diff_added: Color::Rgb(100, 255, 100),
                diff_removed: Color::Rgb(255, 100, 100),
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

pub fn logo() -> String {
    shai_logo().replace("\n", "\r\n")
}

pub fn logo_cyan() -> String {
    let logo = shai_logo().replace("\n", "\r\n");
    apply_gradient(&logo, (255, 0, 255), (0, 255, 255))
}
