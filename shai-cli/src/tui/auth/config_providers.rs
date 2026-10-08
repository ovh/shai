use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Style},
    symbols::border,
    text::{Line, Span, Text},
    widgets::{Block, Borders, Padding, Paragraph},
    Frame,
};
use shai_core::config::config::ShaiConfig;
use shai_llm::provider::ProviderInfo;
use std::io;

use super::auth::NavAction;

#[derive(Debug)]
pub struct ModalProviders {
    config: ShaiConfig,
    providers: Vec<ProviderInfo>,
    selected_provider: usize,
}

impl ModalProviders {
    pub fn new(config: ShaiConfig, providers: Vec<ProviderInfo>) -> Self {
        Self {
            config,
            providers,
            selected_provider: 0,
        }
    }

    pub fn extract_state(self) -> (ShaiConfig, Vec<ProviderInfo>, ProviderInfo) {
        let selected_provider = self.providers[self.selected_provider].clone();
        (self.config, self.providers, selected_provider)
    }
}

impl ModalProviders {
    pub async fn handle_event(&mut self, key_event: KeyEvent) -> NavAction {
        match key_event.code {
            KeyCode::Up => {
                if self.selected_provider > 0 {
                    self.selected_provider -= 1;
                }
            }
            KeyCode::Down => {
                if self.selected_provider < self.providers.len() - 1 {
                    self.selected_provider += 1;
                }
            }
            KeyCode::Enter => return NavAction::Next,
            KeyCode::Esc => return NavAction::Back,
            _ => {}
        }
        NavAction::None
    }

    pub fn draw(&self, frame: &mut Frame, area: Rect) {
        let [list, help] = Layout::vertical(vec![
            Constraint::Length(2 + self.providers.len() as u16),
            Constraint::Length(1),
        ])
        .areas(area);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_set(border::ROUNDED)
            .padding(Padding {
                left: 1,
                right: 1,
                top: 0,
                bottom: 0,
            })
            .title(" Select AI Provider ")
            .style(Style::default().fg(Color::DarkGray));

        let mut lines = vec![];
        for (i, provider) in self.providers.iter().enumerate() {
            let prefix = if i == self.selected_provider {
                "● "
            } else {
                "○ "
            };
            let line = format!("{}{}", prefix, provider.name);

            if i == self.selected_provider {
                lines.push(Line::from(vec![Span::styled(
                    line,
                    Style::default().fg(Color::Green),
                )]));
            } else {
                lines.push(Line::from(vec![Span::styled(
                    line,
                    Style::default().fg(Color::DarkGray),
                )]));
            }
        }

        let text = Text::from(lines);
        let paragraph = Paragraph::new(text).block(block);
        frame.render_widget(paragraph, list);

        frame.render_widget(
            Line::from(vec![Span::styled(
                " ↑↓ navigate • Enter select • Esc exit",
                Style::default().fg(Color::DarkGray),
            )]),
            help,
        );
    }
}
