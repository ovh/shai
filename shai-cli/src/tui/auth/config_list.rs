use std::io;
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

use super::auth::NavAction;
use shai_llm::client::LlmClient;


#[derive(Debug)]
pub struct ModalConfig {
    config: ShaiConfig,
    selected_index: usize,
    error_message: Option<String>,
}

impl ModalConfig {
    pub fn new() -> Self {
        let config = ShaiConfig::load()
            .unwrap_or_default();

        Self {
            config,
            selected_index: 0,
            error_message: None,
        }
    }


    pub fn get_config(&self) -> &ShaiConfig {
        &self.config
    }

    fn total_items(&self) -> usize {
        self.config.providers.len() + 1 // +1 for "Add provider" option
    }

    fn is_add_provider_selected(&self) -> bool {
        self.selected_index == self.config.providers.len()
    }
}

impl ModalConfig {
    pub async fn handle_event(&mut self, key_event: KeyEvent) -> NavAction {
        // Clear any error message on any key press
        self.error_message = None;

        match key_event.code {
            KeyCode::Up => {
                if self.selected_index > 0 {
                    self.selected_index -= 1;
                }
                NavAction::None
            }
            KeyCode::Down => {
                if self.selected_index < self.total_items() - 1 {
                    self.selected_index += 1;
                }
                NavAction::None
            }
            KeyCode::Enter => {
                if self.is_add_provider_selected() {
                    // Start the provider selection flow
                    NavAction::Next
                } else {
                    // Select existing provider and save config
                    if let Err(e) = self.config.set_selected_provider(self.selected_index) {
                        self.error_message = Some(format!("Error selecting provider: {}", e));
                        return NavAction::None;
                    }
                    
                    if let Err(e) = self.config.save() {
                        self.error_message = Some(format!("Error saving config: {}", e));
                        return NavAction::None;
                    }
                    
                    self.config.set_env_vars();
                    NavAction::Done
                }
            }
            KeyCode::Esc => {
                NavAction::Done
            }
            KeyCode::Backspace | KeyCode::Char('d') => {
                // Delete the selected provider (only if it's not the "Add provider" option and we have providers)
                if !self.is_add_provider_selected() && !self.config.providers.is_empty() {
                    if let Err(e) = self.config.remove_provider(self.selected_index) {
                        self.error_message = Some(format!("Error removing provider: {}", e));
                        return NavAction::None;
                    }
                    
                    // Adjust selected_index if needed
                    if self.selected_index >= self.total_items() {
                        self.selected_index = if self.total_items() > 0 { self.total_items() - 1 } else { 0 };
                    }
                    
                    // Save config after deletion
                    if let Err(e) = self.config.save() {
                        self.error_message =
                            Some(format!("Error saving config after deletion: {}", e));
                    }
                }
                NavAction::None
            }
            _ => NavAction::None
        }
    }

    pub fn height(&self) -> usize {
        // 3 for border + title + help, then 1 line per provider + 1 empty line + 1 for "add provider"
        // If there's an error, add 2 more
        let base_height = 4 + self.total_items() + 1 + 1;
        let error_height = if self.error_message.is_some() { 2 } else { 0 };
        base_height + error_height
    }

    pub fn draw(&self, frame: &mut Frame, area: Rect) {
        let mut constraints = vec![Constraint::Length((4 + 1 + self.total_items() + 1) as u16)];

        // Add error area if error message exists
        if self.error_message.is_some() {
            constraints.push(Constraint::Length(2));
        }

        constraints.push(Constraint::Length(1)); // help line

        let layout_areas = Layout::vertical(constraints).split(area);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_set(border::ROUNDED)
            .padding(Padding { left: 1, right: 1, top: 1, bottom: 1 })
            .title(" Select a Providers ")
            .style(Style::default().fg(Color::DarkGray));

        let mut lines = vec![];
        
        // Show existing providers
        for (i, provider_config) in self.config.providers.iter().enumerate() {
            let prefix = if i == self.selected_index { "● " } else { "○ " };
            let selected_indicator = if i == self.config.selected_provider { " (current)" } else { "" };
            let line = format!("{}{} - {}{}", prefix, provider_config.provider, provider_config.model, selected_indicator);
            
            if i == self.selected_index {
                lines.push(Line::from(vec![
                    Span::styled(line, Style::default().fg(Color::Green))
                ]));
            } else if i == self.config.selected_provider {
                lines.push(Line::from(vec![
                    Span::styled(line, Style::default().fg(Color::Cyan))
                ]));
            } else {
                lines.push(Line::from(vec![
                    Span::styled(line, Style::default().fg(Color::DarkGray))
                ]));
            }
        }
        
        // Add empty line separator
        lines.push(Line::from(""));
        
        // Add the "Add provider" option
        let add_line = "+ Add new provider";
        
        if self.is_add_provider_selected() {
            lines.push(Line::from(vec![
                Span::styled(add_line, Style::default().fg(Color::Green).add_modifier(ratatui::style::Modifier::BOLD))
            ]));
        } else {
            lines.push(Line::from(vec![
                Span::styled(add_line, Style::default().fg(Color::DarkGray))
            ]));
        }
        
        let text = Text::from(lines);
        let paragraph = Paragraph::new(text).block(block);
        frame.render_widget(paragraph, layout_areas[0]);

        // Draw error message if present
        if let Some(error) = &self.error_message {
            let error_area_index = 1;
            if let Some(error_area) = layout_areas.get(error_area_index) {
                frame.render_widget(
                    Paragraph::new(error.clone()).style(Style::default().fg(Color::Red)),
                    *error_area,
                );
            }
        }

        // Draw help text
        let help_area_index = if self.error_message.is_some() { 2 } else { 1 };
        if let Some(help_area) = layout_areas.get(help_area_index) {
            let help_text = if self.config.providers.is_empty() {
                " ↑↓ navigate • Enter add provider • Esc exit"
            } else {
                " ↑↓ navigate • Enter select/add • Backspace/d delete • Esc exit"
            };

            frame.render_widget(
                Line::from(vec![Span::styled(
                    help_text,
                    Style::default().fg(Color::DarkGray),
                )]),
                *help_area,
            );
        }
    }

}