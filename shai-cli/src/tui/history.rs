use std::collections::VecDeque;

use ansi_to_tui::IntoText;
use ratatui::{
    layout::Rect,
    widgets::{Paragraph, Widget, Wrap},
    Frame,
};

/// Maximum number of lines stored in the scrollback buffer
const MAX_SCROLLBACK_LINES: usize = 5000;

/// A single line in the conversation history
#[derive(Clone)]
struct ConversationLine {
    text: String,
}

pub struct ConversationHistory {
    /// All lines rendered so far (ANSI-formatted)
    lines: VecDeque<ConversationLine>,
    /// Scroll offset from the bottom (0 = latest)
    scroll_offset: usize,
    /// Last known visible height (updated in draw)
    visible_height: usize,
}

impl ConversationHistory {
    pub fn new() -> Self {
        Self {
            lines: VecDeque::with_capacity(MAX_SCROLLBACK_LINES),
            scroll_offset: 0,
            visible_height: 0,
        }
    }

    /// Add rendered text lines to the history
    pub fn add_text(&mut self, text: &str) {
        for line in text.lines() {
            self.lines.push_back(ConversationLine {
                text: line.to_string(),
            });
            if self.lines.len() > MAX_SCROLLBACK_LINES {
                self.lines.pop_front();
            }
        }
    }

    /// Add a system message (e.g. command output) with a diamond marker and spacing
    pub fn add_system_text(&mut self, text: &str) {
        self.lines.push_back(ConversationLine {
            text: String::new(),
        });
        if self.lines.len() > MAX_SCROLLBACK_LINES {
            self.lines.pop_front();
        }
        for (i, line) in text.lines().enumerate() {
            let text = if i == 0 {
                format!("\x1b[33m\u{25C6}\x1b[0m {}", line)
            } else {
                line.to_string()
            };
            self.lines.push_back(ConversationLine { text });
            if self.lines.len() > MAX_SCROLLBACK_LINES {
                self.lines.pop_front();
            }
        }
    }

    /// Return all history lines as plain text with ANSI escape codes stripped
    pub fn raw_text(&self) -> String {
        self.lines
            .iter()
            .map(|l| strip_ansi(&l.text))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Scroll up by `n` lines
    pub fn scroll_up(&mut self, n: usize) {
        let max_scroll = self.lines.len().saturating_sub(self.visible_height.max(1));
        self.scroll_offset = (self.scroll_offset + n).min(max_scroll);
    }

    /// Scroll down by `n` lines
    pub fn scroll_down(&mut self, n: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(n);
    }

    /// Reset scroll to bottom (latest)
    pub fn scroll_to_bottom(&mut self) {
        self.scroll_offset = 0;
    }

    /// Clear all history lines
    pub fn clear(&mut self) {
        self.lines.clear();
        self.scroll_offset = 0;
    }

    /// Check if scrolled to bottom
    #[allow(dead_code)] // used by tests
    pub fn at_bottom(&self) -> bool {
        self.scroll_offset == 0
    }

    /// Render the conversation history into the given area
    pub fn draw(&mut self, f: &mut Frame, area: Rect) {
        if self.lines.is_empty() {
            return;
        }

        let visible_height = area.height as usize;
        self.visible_height = visible_height;
        if visible_height == 0 {
            return;
        }

        // Build the combined text — newest lines are at the bottom.
        // We render all lines and use Paragraph::scroll to shift the viewport
        // up by scroll_offset lines from the bottom.
        let start = 0;
        let end = self.lines.len();
        let combined: String = self
            .lines
            .range(start..end)
            .map(|l| l.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");

        if let Ok(text) = combined.into_text() {
            // Calculate total rendered rows accounting for line wrapping.
            // Each logical line may occupy multiple rendered rows when wrapped
            // to the area width.
            let width = area.width as usize;
            let total_rendered_rows: usize = if width == 0 {
                self.lines.len()
            } else {
                text.lines
                    .iter()
                    .map(|line| {
                        let w = line.width();
                        if w == 0 {
                            1
                        } else {
                            w.div_ceil(width).max(1)
                        }
                    })
                    .sum()
            };

            // Calculate how many rows to skip from the top.
            // scroll_offset = 0 means showing the latest lines (bottom).
            let skip_from_top = total_rendered_rows
                .saturating_sub(self.scroll_offset + visible_height)
                .min(u16::MAX as usize);

            let paragraph = Paragraph::new(text)
                .wrap(Wrap { trim: false })
                .scroll((skip_from_top as u16, 0));
            f.render_widget(paragraph, area);
        } else {
            let total_lines = self.lines.len();
            let skip_from_top = total_lines
                .saturating_sub(self.scroll_offset + visible_height)
                .min(u16::MAX as usize);
            let paragraph = Paragraph::new(combined)
                .wrap(Wrap { trim: false })
                .scroll((skip_from_top as u16, 0));
            f.render_widget(paragraph, area);
        }
    }
}

/// Strip ANSI escape sequences (SGR and simple CSI) from a string
fn strip_ansi(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
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
            result.push(ch);
        }
    }
    result
}
