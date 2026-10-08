// SPDX-License-Identifier: Apache-2.0
// SPDX-FileCopyrightText: OVH SAS

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Padding},
    Frame,
};
use shai_core::session::SessionData;

use super::theme::ThemePalette;

/// Result of the session picker modal.
pub enum SessionPickerAction {
    /// User selected a session by index into the provided list.
    Selected(usize),
    /// User cancelled (Esc / Ctrl+C).
    Cancelled,
}

pub struct SessionPicker {
    sessions: Vec<SessionData>,
    selected: usize,
    scroll_offset: usize,
    palette: ThemePalette,
}

impl SessionPicker {
    pub fn new(sessions: Vec<SessionData>, palette: ThemePalette) -> Self {
        Self {
            sessions,
            selected: 0,
            scroll_offset: 0,
            palette,
        }
    }

    /// Handle a key event. Returns `Some(SessionPickerAction)` if the picker
    /// should close, or `None` to keep it open.
    pub fn handle_key_event(&mut self, key: KeyEvent) -> Option<SessionPickerAction> {
        if key.kind != KeyEventKind::Press {
            return None;
        }

        match key.code {
            KeyCode::Up => {
                if self.selected > 0 {
                    self.selected -= 1;
                }
            }
            KeyCode::Down => {
                if self.selected + 1 < self.sessions.len() {
                    self.selected += 1;
                }
            }
            KeyCode::PageUp => {
                self.selected = self.selected.saturating_sub(10);
            }
            KeyCode::PageDown => {
                self.selected = (self.selected + 10).min(self.sessions.len().saturating_sub(1));
            }
            KeyCode::Home => {
                self.selected = 0;
            }
            KeyCode::End => {
                self.selected = self.sessions.len().saturating_sub(1);
            }
            KeyCode::Enter => {
                if !self.sessions.is_empty() {
                    return Some(SessionPickerAction::Selected(self.selected));
                }
            }
            KeyCode::Esc => {
                return Some(SessionPickerAction::Cancelled);
            }
            _ => {}
        }

        None
    }

    pub fn draw(&mut self, frame: &mut Frame, area: Rect) {
        let chunks = Layout::vertical([
            Constraint::Length(1), // title
            Constraint::Min(1),    // list
        ])
        .split(area);

        // Title
        let title = Line::from(vec![Span::styled(
            " Restore Session ",
            Style::default()
                .fg(self.palette.suggestion_selected_fg)
                .bold(),
        )]);
        frame.render_widget(title, chunks[0]);

        if self.sessions.is_empty() {
            let empty = Line::from(Span::styled(
                " No saved sessions found.",
                Style::default().fg(self.palette.placeholder),
            ));
            frame.render_widget(empty, chunks[1]);
            return;
        }

        // Account for: title (1) + top border (1) + bottom border (1) = 3
        let max_visible = area.height.saturating_sub(3) as usize;
        let max_visible = max_visible.max(1);

        // Adjust scroll_offset so the selected item is always visible
        if self.sessions.len() <= max_visible {
            self.scroll_offset = 0;
        } else if self.selected < self.scroll_offset {
            self.scroll_offset = self.selected;
        } else if self.selected >= self.scroll_offset + max_visible {
            self.scroll_offset = self.selected - max_visible + 1;
        }

        let visible_count = self.sessions.len().min(max_visible);
        let items: Vec<ListItem> = self
            .sessions
            .iter()
            .skip(self.scroll_offset)
            .take(visible_count)
            .enumerate()
            .map(|(i, session)| {
                let idx = self.scroll_offset + i;
                let name = session.name.as_deref().unwrap_or("unnamed");
                let id_short = &session.session_id[..8.min(session.session_id.len())];

                let line = Line::from(vec![
                    Span::styled(
                        format!("{:>2}. ", idx + 1),
                        Style::default().fg(self.palette.placeholder),
                    ),
                    Span::styled(
                        format!("{:<50} ", name),
                        Style::default().fg(self.palette.input_text),
                    ),
                    Span::styled(
                        format!("({})", id_short),
                        Style::default().fg(self.palette.placeholder),
                    ),
                ]);

                if idx == self.selected {
                    ListItem::new(line)
                        .style(Style::default().bg(self.palette.suggestion_selected_bg))
                } else {
                    ListItem::new(line)
                }
            })
            .collect();

        let list = List::new(items).block(
            Block::default()
                .borders(Borders::ALL)
                .border_set(ratatui::symbols::border::ROUNDED)
                .border_style(Style::default().fg(self.palette.border))
                .bg(self.palette.background)
                .padding(Padding::new(1, 1, 0, 0)),
        );

        frame.render_widget(list, chunks[1]);
    }
}
