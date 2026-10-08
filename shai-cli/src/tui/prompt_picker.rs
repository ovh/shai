// SPDX-License-Identifier: Apache-2.0
// SPDX-FileCopyrightText: OVH SAS

use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::{
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen},
};
use futures::StreamExt;
use ratatui::{
    layout::{Constraint, Layout, Rect},
    prelude::CrosstermBackend,
    style::{Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Padding},
    Frame, Terminal,
};
use shai_core::tools::prompts::PromptInfo;
use std::io::{self, stdout, Write};

use super::modal::run_alternate_screen;
use super::theme::ThemePalette;

/// Result of the prompt picker modal.
pub enum PromptPickerAction {
    /// User confirmed the selection. Contains the list of active prompt names.
    Selected(Vec<String>),
    /// User cancelledleld (Esc / Ctrl+C).
    Cancelled,
}

struct PromptPickerState {
    prompts: Vec<PromptInfo>,
    /// Indices of active prompts.
    active: Vec<usize>,
    selected: usize,
    scroll_offset: usize,
}

impl PromptPickerState {
    fn new(prompts: Vec<PromptInfo>, active: &[String]) -> Self {
        let active_indices = prompts
            .iter()
            .enumerate()
            .filter_map(|(i, p)| {
                if active.iter().any(|a| a == &p.name) {
                    Some(i)
                } else {
                    None
                }
            })
            .collect();
        Self {
            prompts,
            active: active_indices,
            selected: 0,
            scroll_offset: 0,
        }
    }

    fn toggle(&mut self, idx: usize) {
        if let Some(pos) = self.active.iter().position(|&x| x == idx) {
            self.active.remove(pos);
        } else {
            self.active.push(idx);
        }
    }

    fn is_active(&self, idx: usize) -> bool {
        self.active.contains(&idx)
    }

    fn selected_names(&self) -> Vec<String> {
        self.active
            .iter()
            .filter_map(|&i| self.prompts.get(i).map(|p| p.name.clone()))
            .collect()
    }
}

pub struct PromptPicker {
    state: PromptPickerState,
    palette: ThemePalette,
}

impl PromptPicker {
    pub fn new(prompts: Vec<PromptInfo>, active: &[String], palette: ThemePalette) -> Self {
        Self {
            state: PromptPickerState::new(prompts, active),
            palette,
        }
    }

    pub fn render(&mut self, frame: &mut Frame, _area: Rect) {
        let area = frame.area();

        let chunks = Layout::vertical([
            Constraint::Length(1), // title
            Constraint::Min(1),    // list
        ])
        .split(area);

        // Title
        let title = Line::from(vec![Span::styled(
            " System Prompts (Space to toggle, Enter to confirm) ",
            Style::default()
                .fg(self.palette.suggestion_selected_fg)
                .bold(),
        )]);
        frame.render_widget(title, chunks[0]);

        if self.state.prompts.is_empty() {
            let empty = Line::from(Span::styled(
                " No system prompts found. Add .md files to .shai/prompts/ or ~/.config/shai/prompts/",
                Style::default().fg(self.palette.placeholder),
            ));
            frame.render_widget(empty, chunks[1]);
            return;
        }

        let max_visible = area.height.saturating_sub(2) as usize;
        let selected = self.state.selected;

        // Adjust scroll_offset so the selected item is always visible
        let scroll_offset = if self.state.prompts.len() <= max_visible {
            0
        } else if selected < self.state.scroll_offset {
            selected
        } else if selected >= self.state.scroll_offset + max_visible {
            selected - max_visible + 1
        } else {
            self.state.scroll_offset
        };

        let end = (scroll_offset + max_visible).min(self.state.prompts.len());
        let visible_range = scroll_offset..end;

        let items: Vec<ListItem> = visible_range
            .clone()
            .map(|i| {
                let prompt = &self.state.prompts[i];
                let check = if self.state.is_active(i) {
                    "[x]"
                } else {
                    "[ ]"
                };
                let label = if prompt.description.is_empty() {
                    format!("{} {}", check, prompt.name)
                } else {
                    format!("{} {} - {}", check, prompt.name, prompt.description)
                };
                ListItem::new(label)
            })
            .collect();

        let list = List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(self.palette.border))
                    .padding(Padding::horizontal(1)),
            )
            .highlight_style(
                Style::default()
                    .fg(self.palette.suggestion_selected_fg)
                    .bg(self.palette.suggestion_selected_bg),
            );

        // Manually highlight the selected item
        let highlight_idx = selected.saturating_sub(scroll_offset);
        let mut list_state = ratatui::widgets::ListState::default();
        list_state.select(Some(highlight_idx));
        frame.render_stateful_widget(list, chunks[1], &mut list_state);
    }

    pub async fn run(&mut self) -> io::Result<PromptPickerAction> {
        run_alternate_screen(self).await
    }
}

impl crate::tui::modal::Modal for PromptPicker {
    type Output = PromptPickerAction;

    fn draw(&mut self, frame: &mut Frame, area: Rect) {
        self.render(frame, area);
    }

    fn handle_key_event(&mut self, key: crossterm::event::KeyEvent) -> Option<Self::Output> {
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            return Some(PromptPickerAction::Cancelled);
        }
        match key.code {
            KeyCode::Up => {
                if self.state.selected > 0 {
                    self.state.selected -= 1;
                }
            }
            KeyCode::Down => {
                if self.state.selected + 1 < self.state.prompts.len() {
                    self.state.selected += 1;
                }
            }
            KeyCode::PageUp => {
                self.state.selected = self.state.selected.saturating_sub(10);
            }
            KeyCode::PageDown => {
                self.state.selected =
                    (self.state.selected + 10).min(self.state.prompts.len().saturating_sub(1));
            }
            KeyCode::Home => {
                self.state.selected = 0;
            }
            KeyCode::End => {
                self.state.selected = self.state.prompts.len().saturating_sub(1);
            }
            KeyCode::Char(' ') => {
                if !self.state.prompts.is_empty() {
                    self.state.toggle(self.state.selected);
                }
            }
            KeyCode::Enter => {
                return Some(PromptPickerAction::Selected(self.state.selected_names()));
            }
            KeyCode::Esc => {
                return Some(PromptPickerAction::Cancelled);
            }
            _ => {}
        }
        None
    }
}
