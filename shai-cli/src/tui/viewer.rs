use ansi_to_tui::IntoText;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::LeaveAlternateScreen;
use futures::StreamExt;
use ratatui::layout::Rect;
use ratatui::prelude::CrosstermBackend;
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Text};
use ratatui::widgets::{Block, Borders, Padding, Paragraph, Wrap};
use ratatui::Frame as RataFrame;
use ratatui::Terminal;
use shai_core::tools::highlight::highlight_content;
use std::io::{self, stdout};

use super::modal::run_alternate_screen;

/// An expandable full-screen viewer for tool output with syntax highlighting.
///
/// Renders the given content in an alternate terminal screen with scroll support.
/// The user can navigate with arrow keys, Page Up/Down, and Home/End.
/// Press Escape or 'q' to exit.
pub struct AlternateScreenViewer {
    content: String,
    file_path: Option<String>,
    scroll_offset: usize,
}

impl AlternateScreenViewer {
    pub fn new(content: String, file_path: Option<String>) -> Self {
        Self {
            content,
            file_path,
            scroll_offset: 0,
        }
    }

    pub async fn run(&mut self) -> io::Result<()> {
        run_alternate_screen(self).await
    }

    pub fn render(&mut self, frame: &mut RataFrame, _area: Rect) {
        let area = frame.area();

        let display_content = match &self.file_path {
            Some(path) => highlight_content(&self.content, path),
            None => self.content.clone(),
        };

        let title = match &self.file_path {
            Some(path) => format!(" {} ", path),
            None => " Tool Output ".to_string(),
        };

        let text = display_content.into_text().unwrap_or_else(|_| {
            Text::styled(
                display_content.to_string(),
                Style::default().fg(Color::White),
            )
        });
        let total_lines = text.lines.len();

        // Lay out the frame first so we know the visible height, then build the
        // footer (which carries the scroll %) into the bottom border line.
        //
        // Only TOP/BOTTOM borders are drawn: the left/right border characters
        // would otherwise be copied along with the content when the user selects
        // multiple lines (the whole point of this raw viewer). Horizontal padding
        // is likewise zero so copied lines are flush.
        let block = Block::default()
            .borders(Borders::TOP | Borders::BOTTOM)
            .padding(Padding::new(0, 0, 1, 1));
        let inner = block.inner(area);
        let visible_height = inner.height as usize;

        let clamped_offset = if total_lines > visible_height {
            self.scroll_offset
                .min(total_lines.saturating_sub(visible_height))
        } else {
            0
        };

        // Scroll position as a percentage, shown in the bottom border so selecting
        // content never picks up extra UI glyphs.
        let max_offset = total_lines.saturating_sub(visible_height);
        let scroll_hint = clamped_offset
            .checked_mul(100)
            .and_then(|v| v.checked_div(max_offset))
            .map(|percent| format!(" | {}%", percent))
            .unwrap_or_default();
        let footer = format!(
            " \u{2191}\u{2193} Scroll | PageUp/PageDown | q/Esc Close{}",
            scroll_hint
        );

        let block = block
            .border_style(Style::default().fg(Color::Cyan))
            .title(Line::from(title.trim()).add_modifier(Modifier::BOLD))
            .title_bottom(Line::from(footer).fg(Color::DarkGray));
        frame.render_widget(block, area);

        let paragraph = Paragraph::new(text)
            .wrap(Wrap { trim: false })
            .scroll((clamped_offset as u16, 0));
        frame.render_widget(paragraph, inner);
    }
}

impl crate::tui::modal::Modal for AlternateScreenViewer {
    type Output = ();

    fn draw(&mut self, frame: &mut RataFrame, area: Rect) {
        self.render(frame, area);
    }

    fn handle_key_event(&mut self, key_event: crossterm::event::KeyEvent) -> Option<Self::Output> {
        match key_event.code {
            KeyCode::Char('c') if key_event.modifiers.contains(KeyModifiers::CONTROL) => Some(()),
            KeyCode::Char('q') | KeyCode::Esc => Some(()),
            KeyCode::Up => {
                self.scroll_offset = self.scroll_offset.saturating_sub(1);
                None
            }
            KeyCode::Down => {
                self.scroll_offset = self.scroll_offset.saturating_add(1);
                None
            }
            KeyCode::PageUp => {
                self.scroll_offset = self.scroll_offset.saturating_sub(20);
                None
            }
            KeyCode::PageDown => {
                self.scroll_offset = self.scroll_offset.saturating_add(20);
                None
            }
            KeyCode::Home => {
                self.scroll_offset = 0;
                None
            }
            KeyCode::End => {
                self.scroll_offset = self.content.lines().count();
                None
            }
            _ => None,
        }
    }
}

impl Drop for AlternateScreenViewer {
    fn drop(&mut self) {
        let _ = execute!(stdout(), LeaveAlternateScreen);
    }
}
