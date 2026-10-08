use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    symbols::border,
    text::{Line, Span, Text},
    widgets::{Block, Borders, Padding, Paragraph},
    Frame,
};
use shai_core::config::config::ShaiConfig;
use shai_llm::provider::ProviderInfo;
use std::{collections::HashMap, io};

use super::auth::NavAction;

#[derive(Debug)]
pub struct ModalModel {
    pub all_models: Vec<String>,
    pub filtered_models: Vec<String>,
    pub selected_index: usize,
    pub scroll_offset: usize,
    pub search_query: String,
    pub search_mode: bool,
    pub config: ShaiConfig,
    pub providers: Vec<ProviderInfo>,
    pub provider: ProviderInfo,
    pub env_values: HashMap<String, String>,
    pub error_message: Option<String>,
}

const MAX_VISIBLE_MODELS: usize = 20;
const SCROLL_MARGIN: usize = 10;

impl ModalModel {
    pub fn new(
        available_models: Vec<String>,
        config: ShaiConfig,
        providers: Vec<ProviderInfo>,
        provider: ProviderInfo,
        env_values: HashMap<String, String>,
    ) -> Self {
        let filtered_models = available_models.clone();
        Self {
            all_models: available_models,
            filtered_models,
            selected_index: 0,
            scroll_offset: 0,
            search_query: String::new(),
            search_mode: false,
            config,
            providers,
            provider,
            env_values,
            error_message: None,
        }
    }

    pub fn selected_model(&self) -> String {
        if self.selected_index < self.filtered_models.len() {
            self.filtered_models[self.selected_index].clone()
        } else {
            String::new()
        }
    }

    fn filter_models(&mut self) {
        if self.search_query.is_empty() {
            self.filtered_models = self.all_models.clone();
        } else {
            let query = self.search_query.to_lowercase();
            self.filtered_models = self
                .all_models
                .iter()
                .filter(|model| model.to_lowercase().contains(&query))
                .cloned()
                .collect();
        }

        // Reset selection if it's out of bounds
        if self.selected_index >= self.filtered_models.len() && !self.filtered_models.is_empty() {
            self.selected_index = 0;
            self.scroll_offset = 0;
        }
    }

    fn update_scroll(&mut self) {
        if self.filtered_models.len() <= MAX_VISIBLE_MODELS {
            self.scroll_offset = 0;
            return;
        }

        // Keep selection in the middle range when possible
        let middle_start = SCROLL_MARGIN;

        if self.selected_index < middle_start {
            // Near the top, show from beginning
            self.scroll_offset = 0;
        } else if self.selected_index > self.filtered_models.len() - SCROLL_MARGIN - 1 {
            // Near the bottom, show to the end
            self.scroll_offset = self.filtered_models.len() - MAX_VISIBLE_MODELS;
        } else {
            // In the middle, center the selection
            self.scroll_offset = self.selected_index - SCROLL_MARGIN;
        }
    }
}

impl ModalModel {
    pub async fn handle_event(&mut self, key_event: KeyEvent) -> NavAction {
        match key_event.code {
            KeyCode::Esc => {
                if self.search_mode {
                    self.search_mode = false;
                    self.search_query.clear();
                    self.filter_models();
                    self.error_message = None;
                } else {
                    return NavAction::Back;
                }
            }
            KeyCode::Enter => {
                if self.filtered_models.is_empty() {
                    return NavAction::None;
                }
                let selected_model = &self.filtered_models[self.selected_index];
                if self.config.is_duplicate_config(
                    self.provider.name,
                    &self.env_values,
                    selected_model,
                ) {
                    self.error_message =
                        Some("This instance already exists in the configuration".to_string());
                    return NavAction::None;
                }
                return NavAction::Done;
            }
            KeyCode::Backspace if self.search_mode => {
                self.search_query.pop();
                self.filter_models();
                self.update_scroll();
                self.error_message = None;
            }
            KeyCode::Char(c) => {
                if !self.search_mode {
                    self.search_mode = true;
                    self.search_query.clear();
                }
                self.search_query.push(c);
                self.filter_models();
                self.update_scroll();
                self.error_message = None;
            }
            KeyCode::Up => {
                if self.selected_index > 0 {
                    self.selected_index -= 1;
                    self.update_scroll();
                    self.error_message = None;
                }
            }
            KeyCode::Down if self.selected_index + 1 < self.filtered_models.len() => {
                self.selected_index += 1;
                self.update_scroll();
                self.error_message = None;
            }
            _ => {}
        }
        NavAction::None
    }

    fn inner_height(&self) -> usize {
        let search_bar_height = if self.search_mode { 2 } else { 0 };
        let visible_models = std::cmp::min(self.filtered_models.len(), MAX_VISIBLE_MODELS);
        let error_height = if self.error_message.is_some() { 2 } else { 0 };
        let help_height = 2;
        visible_models + search_bar_height + error_height + help_height
    }

    pub fn draw(&self, frame: &mut Frame, area: Rect) {
        use ratatui::layout::{Constraint, Layout};

        let title = if self.search_mode {
            format!(" Select Model (Search: {}) ", self.search_query)
        } else {
            " Select Model ".to_string()
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_set(border::ROUNDED)
            .padding(Padding {
                left: 1,
                right: 1,
                top: 1,
                bottom: 1,
            })
            .title(title)
            .style(Style::default().fg(Color::DarkGray));

        let inner = block.inner(area);
        frame.render_widget(block, area);

        let constraints = vec![Constraint::Length(1); self.inner_height()];
        let layout_areas = Layout::vertical(constraints).split(inner);

        let mut area_index = 0;

        // Draw search bar
        if self.search_mode {
            let search_text = format!("Search: {}", self.search_query);
            let search_paragraph =
                Paragraph::new(search_text).style(Style::default().fg(Color::Yellow));
            frame.render_widget(search_paragraph, layout_areas[area_index]);
            area_index += 2; // Skip search line and empty line
        }

        // Draw visible models
        let end_index = std::cmp::min(
            self.scroll_offset + MAX_VISIBLE_MODELS,
            self.filtered_models.len(),
        );

        for (display_idx, model_idx) in (self.scroll_offset..end_index).enumerate() {
            // Draw top scroll indicator
            if self.filtered_models.len() > MAX_VISIBLE_MODELS
                && display_idx == 0
                && self.scroll_offset > 0
            {
                let remaining_above = self.scroll_offset;
                let scroll_info = format!("... {} more above ...", remaining_above);
                let scroll_paragraph =
                    Paragraph::new(scroll_info).style(Style::default().fg(Color::Blue));
                frame.render_widget(scroll_paragraph, layout_areas[area_index]);
                area_index += 1;
                continue;
            }

            // Draw bottom scroll indicator
            if self.filtered_models.len() > MAX_VISIBLE_MODELS
                && model_idx == end_index - 1
                && self.scroll_offset + MAX_VISIBLE_MODELS < self.filtered_models.len()
            {
                let remaining_below = self.filtered_models.len() - end_index;
                let scroll_info = format!("... {} more below ...", remaining_below);
                let scroll_paragraph =
                    Paragraph::new(scroll_info).style(Style::default().fg(Color::Blue));
                frame.render_widget(scroll_paragraph, layout_areas[area_index]);
                area_index += 1;
                continue;
            }

            let model = &self.filtered_models[model_idx];
            let is_selected = model_idx == self.selected_index;
            let is_duplicate =
                self.config
                    .is_duplicate_config(self.provider.name, &self.env_values, model);

            let prefix = if is_selected { "● " } else { "○ " };
            let line = format!("{}{}", prefix, model);

            let style = if is_duplicate {
                Style::default()
                    .fg(Color::DarkGray)
                    .add_modifier(Modifier::DIM)
            } else if is_selected {
                Style::default().fg(Color::Green)
            } else {
                Style::default().fg(Color::DarkGray)
            };

            let paragraph = Paragraph::new(line).style(style);
            frame.render_widget(paragraph, layout_areas[area_index]);
            area_index += 1;
        }

        // Draw error message if present
        if let Some(error) = &self.error_message {
            area_index += 1; // Skip empty line
            let error_paragraph =
                Paragraph::new(error.clone()).style(Style::default().fg(Color::Red));
            frame.render_widget(error_paragraph, layout_areas[area_index]);
            area_index += 1;
        }

        // Draw help text
        area_index += 1; // Skip empty line
        let help_text = if self.search_mode {
            "Type to search • ↑↓ navigate • Backspace clear • Esc clear search • Enter select"
        } else {
            "↑↓ navigate • Type to search • Enter select • Esc back"
        };
        let help_paragraph = Paragraph::new(help_text).style(Style::default().fg(Color::DarkGray));
        frame.render_widget(help_paragraph, layout_areas[area_index]);
    }
}
