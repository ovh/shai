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
use shai_core::config::agent::AgentConfig;

use super::theme::ThemePalette;

pub enum AgentPickerAction {
    Selected(String),
    Cancelled,
}

pub struct AgentPicker {
    agents: Vec<(String, String)>,
    selected: usize,
    scroll_offset: usize,
    palette: ThemePalette,
}

impl AgentPicker {
    pub fn new(palette: ThemePalette) -> Self {
        let agents = AgentConfig::list_agents()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|name| {
                AgentConfig::load(&name).ok().map(|config| {
                    let desc = if config.description.is_empty() {
                        String::new()
                    } else {
                        config.description
                    };
                    (name, desc)
                })
            })
            .collect();

        Self {
            agents,
            selected: 0,
            scroll_offset: 0,
            palette,
        }
    }

    pub fn handle_key_event(&mut self, key: KeyEvent) -> Option<AgentPickerAction> {
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
                if self.selected + 1 < self.agents.len() {
                    self.selected += 1;
                }
            }
            KeyCode::PageUp => {
                self.selected = self.selected.saturating_sub(10);
            }
            KeyCode::PageDown => {
                self.selected = (self.selected + 10).min(self.agents.len().saturating_sub(1));
            }
            KeyCode::Home => {
                self.selected = 0;
            }
            KeyCode::End => {
                self.selected = self.agents.len().saturating_sub(1);
            }
            KeyCode::Enter => {
                if let Some((name, _)) = self.agents.get(self.selected) {
                    return Some(AgentPickerAction::Selected(name.clone()));
                }
            }
            KeyCode::Esc => {
                return Some(AgentPickerAction::Cancelled);
            }
            _ => {}
        }

        None
    }

    pub fn draw(&mut self, frame: &mut Frame, area: Rect) {
        let chunks = Layout::vertical([
            Constraint::Length(1),
            Constraint::Min(1),
        ])
        .split(area);

        let title = Line::from(vec![Span::styled(
            " Select Agent ",
            Style::default()
                .fg(self.palette.suggestion_selected_fg)
                .bold(),
        )]);
        frame.render_widget(title, chunks[0]);

        if self.agents.is_empty() {
            let empty = Line::from(Span::styled(
                " No custom agents found. Create agent configs in ~/.config/shai/agents/",
                Style::default().fg(self.palette.placeholder),
            ));
            frame.render_widget(empty, chunks[1]);
            return;
        }

        let max_visible = area.height.saturating_sub(3) as usize;
        let max_visible = max_visible.max(1);

        if self.agents.len() <= max_visible {
            self.scroll_offset = 0;
        } else if self.selected < self.scroll_offset {
            self.scroll_offset = self.selected;
        } else if self.selected >= self.scroll_offset + max_visible {
            self.scroll_offset = self.selected - max_visible + 1;
        }

        let visible_count = self.agents.len().min(max_visible);
        let items: Vec<ListItem> = self
            .agents
            .iter()
            .skip(self.scroll_offset)
            .take(visible_count)
            .enumerate()
            .map(|(i, (name, desc))| {
                let idx = self.scroll_offset + i;
                let display_name = if desc.is_empty() {
                    name.clone()
                } else {
                    format!("{:<30} {}", name, desc)
                };
                let line = Line::from(vec![
                    Span::styled(
                        format!("{:>2}. ", idx + 1),
                        Style::default().fg(self.palette.placeholder),
                    ),
                    Span::styled(
                        display_name,
                        Style::default().fg(self.palette.input_text),
                    ),
                ]);

                if idx == self.selected {
                    ListItem::new(line).style(Style::default().bg(self.palette.suggestion_selected_bg))
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
