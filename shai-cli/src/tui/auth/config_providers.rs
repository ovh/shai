use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Style},
    symbols::border,
    text::{Line, Span, Text},
    widgets::{Block, Borders, Padding, Paragraph, Wrap},
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
    error_message: Option<String>,
}

impl ModalProviders {
    pub fn new(config: ShaiConfig, providers: Vec<ProviderInfo>) -> Self {
        Self::from_parts(config, providers, None)
    }

    pub fn new_with_error(config: ShaiConfig, providers: Vec<ProviderInfo>, error: String) -> Self {
        Self::from_parts(config, providers, Some(error))
    }

    fn from_parts(
        config: ShaiConfig,
        providers: Vec<ProviderInfo>,
        error_message: Option<String>,
    ) -> Self {
        Self {
            config,
            providers,
            selected_provider: 0,
            error_message,
        }
    }

    pub fn extract_state(self) -> (ShaiConfig, Vec<ProviderInfo>, ProviderInfo) {
        let selected_provider = self.providers[self.selected_provider].clone();
        (self.config, self.providers, selected_provider)
    }
}

impl ModalProviders {
    pub async fn handle_event(&mut self, key_event: KeyEvent) -> NavAction {
        // Clear any error message on any key press
        self.error_message = None;

        match key_event.code {
            KeyCode::Up => {
                if self.selected_provider > 0 {
                    self.selected_provider -= 1;
                }
            }
            KeyCode::Down => {
                if self.selected_provider + 1 < self.providers.len() {
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
        let mut constraints = vec![Constraint::Length(2 + self.providers.len() as u16)];

        // Add error area if error message exists
        let error_height = self
            .error_message
            .as_deref()
            .map(|error| super::error_height(error, area.width))
            .unwrap_or(0);
        if error_height > 0 {
            constraints.push(Constraint::Length(error_height));
        }

        constraints.push(Constraint::Length(1)); // help line

        let layout_areas = Layout::vertical(constraints).split(area);

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
        frame.render_widget(paragraph, layout_areas[0]);

        // Draw error message if present
        if let Some(error) = &self.error_message {
            if let Some(error_area) = layout_areas.get(1) {
                frame.render_widget(
                    Paragraph::new(error.clone())
                        .style(Style::default().fg(Color::Red))
                        .wrap(Wrap { trim: false }),
                    *error_area,
                );
            }
        }

        // Draw help text
        let help_area_index = if self.error_message.is_some() { 2 } else { 1 };
        if let Some(help_area) = layout_areas.get(help_area_index) {
            frame.render_widget(
                Line::from(vec![Span::styled(
                    " ↑↓ navigate • Enter select • Esc exit",
                    Style::default().fg(Color::DarkGray),
                )]),
                *help_area,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn key_press(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[tokio::test]
    async fn error_message_is_cleared_on_next_key_press() {
        let mut modal =
            ModalProviders::new_with_error(ShaiConfig::default(), vec![], "boom".to_string());
        assert_eq!(modal.error_message.as_deref(), Some("boom"));

        modal.handle_event(key_press(KeyCode::Down)).await;

        assert_eq!(modal.error_message, None);
    }

    #[tokio::test]
    async fn enter_returns_next() {
        let mut modal = ModalProviders::new(ShaiConfig::default(), vec![]);

        let action = modal.handle_event(key_press(KeyCode::Enter)).await;

        assert!(matches!(action, NavAction::Next));
    }
}
