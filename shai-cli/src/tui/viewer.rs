use ansi_to_tui::IntoText;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::LeaveAlternateScreen;
use futures::StreamExt;
use ratatui::layout::Rect;
use ratatui::prelude::CrosstermBackend;
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Text};
use ratatui::widgets::{Block, Borders, Padding, Paragraph, Scrollbar, ScrollbarOrientation, Wrap};
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

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan))
            .title(Line::from(title.trim()).add_modifier(Modifier::BOLD))
            .padding(Padding::new(1, 1, 1, 1));

        let inner = block.inner(area);
        frame.render_widget(block, area);

        let text = display_content.into_text().unwrap_or_else(|_| {
            Text::styled(
                display_content.to_string(),
                Style::default().fg(Color::White),
            )
        });

        let total_lines = text.lines.len();
        let visible_height = inner.height as usize;

        let clamped_offset = if total_lines > visible_height {
            self.scroll_offset
                .min(total_lines.saturating_sub(visible_height))
        } else {
            0
        };

        let paragraph = Paragraph::new(text)
            .wrap(Wrap { trim: false })
            .scroll((clamped_offset as u16, 0));
        frame.render_widget(paragraph, inner);

        if total_lines > visible_height {
            let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .begin_symbol(None)
                .end_symbol(None);
            let mut scrollbar_state =
                ratatui::widgets::ScrollbarState::new(total_lines).position(clamped_offset);
            frame.render_stateful_widget(scrollbar, inner, &mut scrollbar_state);
        }

        let footer_area = Rect::new(area.x, area.bottom().saturating_sub(1), area.width, 1);
        let footer = Paragraph::new(" \u{2191}\u{2193} Scroll | PageUp/PageDown | q/Esc Close")
            .style(Style::default().fg(Color::DarkGray));
        frame.render_widget(footer, footer_area);
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
