use ratatui::{
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::Widget,
    Frame,
};

use super::theme::{Theme, ThemePalette};

/// Format a token count into human-readable form (K, M, G)
fn format_tokens(n: u32) -> String {
    if n < 1000 {
        n.to_string()
    } else if n < 1_000_000 {
        format!("{:.1}K", n as f64 / 1000.0)
    } else if n < 1_000_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else {
        format!("{:.1}G", n as f64 / 1_000_000_000.0)
    }
}

/// Shorten a filesystem path for display in the status bar.
/// Shows `~` at home directory, otherwise `../<current_dir>`.
fn shorten_path(path: &str) -> String {
    let home = std::env::var("HOME").unwrap_or_default();

    if !home.is_empty() && path == home {
        return "~".to_string();
    }

    let components: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    match components.len() {
        0 => "/".to_string(),
        1 => components[0].to_string(),
        _ => format!("../{}", components.last().unwrap()),
    }
}

/// Information displayed in the persistent status bar
#[derive(Clone)]
pub struct StatusBarInfo {
    pub model: String,
    pub provider: String,
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub location: String,
    pub git_branch: String,
    pub agent_mode: String,
    pub tool_call_method: String,
}

/// Minimum interval between git branch refreshes
const GIT_REFRESH_INTERVAL: std::time::Duration = std::time::Duration::from_secs(2);

/// Pure throttle check: refresh when never refreshed or interval elapsed
fn should_refresh(last: Option<std::time::Instant>, now: std::time::Instant) -> bool {
    last.is_none_or(|t| now.duration_since(t) >= GIT_REFRESH_INTERVAL)
}

pub struct StatusBar {
    info: StatusBarInfo,
    theme: Theme,
    notification: Option<String>,
    notification_until: Option<std::time::Instant>,
    last_git_refresh: Option<std::time::Instant>,
}

impl StatusBar {
    pub fn new(theme: Theme) -> Self {
        Self {
            info: StatusBarInfo {
                model: String::new(),
                provider: String::new(),
                input_tokens: 0,
                output_tokens: 0,
                location: String::new(),
                git_branch: String::new(),
                agent_mode: String::new(),
                tool_call_method: String::new(),
            },
            theme,
            notification: None,
            notification_until: None,
            last_git_refresh: None,
        }
    }

    /// Returns true when the git branch should be refreshed (at most once per
    /// `GIT_REFRESH_INTERVAL`), recording the refresh time.
    pub fn git_needs_refresh(&mut self) -> bool {
        let now = std::time::Instant::now();
        if should_refresh(self.last_git_refresh, now) {
            self.last_git_refresh = Some(now);
            true
        } else {
            false
        }
    }

    pub fn theme(&self) -> &Theme {
        &self.theme
    }

    pub fn theme_mut(&mut self) -> &mut Theme {
        &mut self.theme
    }

    pub fn palette(&self) -> ThemePalette {
        self.theme.palette()
    }

    pub fn set_model(&mut self, model: &str) {
        self.info.model = model.to_string();
    }

    pub fn set_provider(&mut self, provider: &str) {
        self.info.provider = provider.to_string();
    }

    pub fn set_tokens(&mut self, input: u32, output: u32) {
        self.info.input_tokens = input;
        self.info.output_tokens = output;
    }

    pub fn set_location(&mut self, location: &str) {
        self.info.location = location.to_string();
    }

    pub fn set_git_branch(&mut self, branch: &str) {
        self.info.git_branch = branch.to_string();
    }

    pub fn set_agent_mode(&mut self, mode: &str) {
        self.info.agent_mode = mode.to_string();
    }

    pub fn set_tool_call_method(&mut self, method: &str) {
        self.info.tool_call_method = method.to_string();
    }

    pub fn set_notification(&mut self, msg: &str, duration: std::time::Duration) {
        self.notification = Some(msg.to_string());
        self.notification_until = Some(std::time::Instant::now() + duration);
    }

    pub fn draw(&self, f: &mut Frame, area: Rect) {
        let palette = self.theme.palette();
        let primary = Style::default()
            .fg(palette.chip_primary_fg)
            .bg(palette.chip_primary_bg);
        let secondary = Style::default()
            .fg(palette.chip_secondary_fg)
            .bg(palette.chip_secondary_bg);
        let accent = Style::default()
            .fg(palette.chip_accent_fg)
            .bg(palette.chip_accent_bg);
        let warn = Style::default()
            .fg(palette.chip_warn_fg)
            .bg(palette.chip_warn_bg);

        let mut spans = vec![
            Span::styled(format!(" \u{2388} {} ", self.info.provider), primary),
            Span::styled(format!(" \u{2756} {} ", self.info.model), secondary),
        ];

        // Location (shown after model if available)
        if !self.info.location.is_empty() {
            spans.push(Span::styled(
                format!(" \u{2AFD} {} ", shorten_path(&self.info.location)),
                warn,
            ));
        }

        // Git branch (shown after location if available)
        if !self.info.git_branch.is_empty() {
            spans.push(Span::styled(
                format!(" \u{2325} {} ", self.info.git_branch),
                secondary,
            ));
        }

        // Agent mode (shown after git branch)
        if !self.info.agent_mode.is_empty() {
            spans.push(Span::styled(format!(" {} ", self.info.agent_mode), accent));
        }

        // Tool call method (shown only when customized by user)
        if !self.info.tool_call_method.is_empty() {
            spans.push(Span::styled(
                format!(" {} ", self.info.tool_call_method),
                warn,
            ));
        }

        // Notification (shown after agent mode)
        let mut notification_str = String::new();
        if let (Some(msg), Some(until)) = (&self.notification, self.notification_until) {
            if std::time::Instant::now() < until {
                notification_str = format!(" {} ", msg);
            }
        }
        if !notification_str.is_empty() {
            spans.push(Span::styled(notification_str, warn));
        }

        // Right-aligned: tokens
        let total = self.info.input_tokens + self.info.output_tokens;
        let token_str = format!(
            " \u{2211} {} \u{2191}{} \u{2193}{} ",
            format_tokens(total),
            format_tokens(self.info.input_tokens),
            format_tokens(self.info.output_tokens),
        );

        let left_len: usize = spans.iter().map(|s| s.width()).sum();
        let right_len = Span::raw(&token_str).width();
        let padding = area.width as usize;
        let spaces = padding.saturating_sub(left_len + right_len);
        let spacer = " ".repeat(spaces);

        spans.push(Span::styled(
            spacer,
            Style::default().bg(palette.chip_secondary_bg),
        ));
        spans.push(Span::styled(token_str, secondary));

        let line = Line::from(spans);
        f.render_widget(line, area);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn test_should_refresh_when_never_refreshed() {
        assert!(should_refresh(None, Instant::now()));
    }

    #[test]
    fn test_should_refresh_throttles_within_interval() {
        let now = Instant::now();
        let last = now - Duration::from_millis(500);
        assert!(!should_refresh(Some(last), now));
    }

    #[test]
    fn test_should_refresh_after_interval_elapsed() {
        let now = Instant::now();
        let last = now - Duration::from_secs(3);
        assert!(should_refresh(Some(last), now));
    }

    #[test]
    fn test_git_needs_refresh_first_call_then_throttled() {
        let mut bar = StatusBar::new(Theme::Dark);
        assert!(bar.git_needs_refresh());
        assert!(!bar.git_needs_refresh());
    }
}
