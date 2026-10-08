use crossterm::event::{Event, KeyCode, KeyEvent};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Style},
    symbols::border,
    text::{Line, Span, Text},
    widgets::{Block, Borders, Padding, Paragraph},
    Frame,
};
use shai_core::config::config::ShaiConfig;
use shai_llm::client::LlmClient;
use shai_llm::provider::ProviderInfo;
use std::collections::HashMap;
use std::io;
use tokio::task::JoinHandle;
use ratatui_textarea::TextArea;

use super::auth::NavAction;

#[derive(Debug)]
pub enum FetchState {
    Idle,
    Fetching,
    Success,
    Error,
}

#[derive(Debug)]
pub struct ModalEnvs {
    config: ShaiConfig,
    providers: Vec<ProviderInfo>,
    provider: ProviderInfo,
    input_fields: Vec<TextArea<'static>>,
    current_field: usize,
    env_values: HashMap<String, String>,
    error_message: Option<String>,
    fetch_state: FetchState,
    fetch_task: Option<JoinHandle<Result<Vec<String>, String>>>,
}

impl ModalEnvs {
    pub fn new(config: ShaiConfig, providers: Vec<ProviderInfo>, provider: ProviderInfo) -> Self {
        let input_fields = provider
            .env_vars
            .iter()
            .map(|_| TextArea::default())
            .collect();

        Self {
            config,
            providers,
            provider,
            input_fields,
            current_field: 0,
            env_values: HashMap::new(),
            error_message: None,
            fetch_state: FetchState::Idle,
            fetch_task: None,
        }
    }

    pub fn extract_state(
        self,
    ) -> (
        ShaiConfig,
        Vec<ProviderInfo>,
        ProviderInfo,
        HashMap<String, String>,
    ) {
        (self.config, self.providers, self.provider, self.env_values)
    }

    fn start_fetch_models(&mut self) {
        // Clear previous env values and update them
        self.env_values.clear();

        // Set environment variables based on provider's required env vars
        for (i, env_var) in self.provider.env_vars.iter().enumerate() {
            if i < self.input_fields.len() {
                let value = self.input_fields[i].lines().join("\n").trim().to_string();
                if !value.is_empty() {
                    self.env_values.insert(env_var.name.clone(), value);
                }
            }
        }

        // Cancel any existing fetch task
        if let Some(handle) = self.fetch_task.take() {
            handle.abort();
        }

        // Start async fetch
        self.fetch_state = FetchState::Fetching;
        self.error_message = None;

        let provider_name = self.provider.name.to_string();
        let env_values = self.env_values.clone();

        self.fetch_task = Some(tokio::spawn(async move {
            match LlmClient::create_provider(&provider_name, &env_values) {
                Ok(client) => match client.models().await {
                    Ok(models) => Ok(models.data.into_iter().map(|m| m.id).collect()),
                    Err(e) => Err(format!("Failed to fetch models: {}", e)),
                },
                Err(e) => Err(format!("Failed to create client: {}", e)),
            }
        }));
    }

    pub fn is_fetching(&self) -> bool {
        matches!(self.fetch_state, FetchState::Fetching)
    }

    pub fn poll_fetch(&mut self) -> Option<Result<Vec<String>, String>> {
        if self.fetch_task.as_ref()?.is_finished() {
            let task = self.fetch_task.take()?;
            let result =
                futures::executor::block_on(task).unwrap_or_else(|_| Err("Cancelled".to_string()));

            // Update internal state based on result
            match &result {
                Ok(_) => self.fetch_state = FetchState::Success,
                Err(e) => {
                    self.fetch_state = FetchState::Error;
                    self.error_message = Some(format!("Error: {}", e));
                }
            }

            Some(result)
        } else {
            None
        }
    }
}

impl ModalEnvs {
    pub async fn handle_event(&mut self, key_event: KeyEvent) -> NavAction {
        match key_event.code {
            KeyCode::Enter => {
                match &self.fetch_state {
                    FetchState::Fetching => {
                        // Already fetching, ignore
                        NavAction::None
                    }
                    _ => {
                        // Start fetching models
                        self.start_fetch_models();
                        NavAction::None
                    }
                }
            }
            KeyCode::Tab => {
                if !self.input_fields.is_empty() {
                    self.current_field = (self.current_field + 1) % self.input_fields.len();
                }
                NavAction::None
            }
            KeyCode::BackTab => {
                if !self.input_fields.is_empty() {
                    self.current_field = (self.current_field - 1) % self.input_fields.len();
                }
                NavAction::None
            }
            KeyCode::Esc => {
                // Cancel any running fetch task
                if let Some(handle) = self.fetch_task.take() {
                    handle.abort();
                }
                NavAction::Back
            }
            _ => {
                if self.current_field < self.input_fields.len() {
                    let event = ratatui_textarea::Input::from(Event::Key(key_event));
                    self.input_fields[self.current_field].input(event);
                }
                NavAction::None
            }
        }
    }

    pub fn draw(&mut self, frame: &mut Frame, area: Rect) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_set(border::ROUNDED)
            .padding(Padding {
                left: 1,
                right: 1,
                top: 1,
                bottom: 1,
            })
            .title(format!(" Configure {} ", self.provider.name))
            .style(Style::default().fg(Color::DarkGray));

        let inner = block.inner(area);
        frame.render_widget(block, area);

        let env_vars = &self.provider.env_vars;
        let num_fields = env_vars.len();

        // Calculate constraints for dynamic number of fields
        let mut constraints = vec![];
        for _ in 0..num_fields {
            constraints.push(Constraint::Length(3));
            constraints.push(Constraint::Length(1));
        }
        if self.error_message.is_some() {
            constraints.push(Constraint::Length(2)); // Error area
        }
        constraints.push(Constraint::Length(1)); // Help area

        let layout_areas = Layout::vertical(constraints).split(inner);

        // Draw all input fields
        for (i, env_var) in env_vars.iter().enumerate() {
            if i < self.input_fields.len() && i < layout_areas.len() {
                let is_secret = env_var.name.to_lowercase().contains("key")
                    || env_var.name.to_lowercase().contains("secret")
                    || env_var.name.to_lowercase().contains("token");

                let block = Block::default()
                    .borders(Borders::ALL)
                    .border_set(border::ROUNDED)
                    .padding(Padding {
                        left: 1,
                        right: 1,
                        top: 0,
                        bottom: 0,
                    })
                    .title(format!(" {} ", env_var.name))
                    .style(if self.current_field == i {
                        Style::default().fg(Color::White)
                    } else {
                        Style::default().fg(Color::DarkGray)
                    });

                match env_var.name.as_str() {
                    "OVH_BASE_URL" => self.input_fields[i]
                        .set_placeholder_text("https://oai.endpoints.kepler.ai.cloud.ovh.net/v1"),
                    "OLLAMA_BASE_URL" => {
                        self.input_fields[i].set_placeholder_text("http://localhost:11434/v1")
                    }
                    _ => {}
                }

                if is_secret {
                    self.input_fields[i].set_mask_char('\u{2022}');
                }
                self.input_fields[i].set_block(block);
                self.input_fields[i].set_cursor_style(Style::reset());
                self.input_fields[i].set_placeholder_style(Style::default().fg(Color::DarkGray));
                self.input_fields[i].set_style(Style::default().fg(Color::White));
                self.input_fields[i].set_cursor_line_style(Style::default());
                frame.render_widget(&self.input_fields[i], layout_areas[2 * i]);
            }
        }

        // Draw error if present
        if let Some(error) = &self.error_message {
            if let Some(error_area) = layout_areas.get(2 * num_fields) {
                frame.render_widget(
                    Paragraph::new(error.clone()).style(Style::default().fg(Color::Red)),
                    *error_area,
                );
            }
        }

        // Draw help text
        let help_area_index = if self.error_message.is_some() {
            2 * num_fields + 1
        } else {
            2 * num_fields
        };
        if let Some(help_area) = layout_areas.get(help_area_index) {
            let help_text = match &self.fetch_state {
                FetchState::Fetching => "Fetching models... • Esc cancel",
                _ => {
                    if num_fields > 1 {
                        "Type value • Tab switch • Enter confirm • Esc back"
                    } else {
                        "Type value • Enter confirm • Esc back"
                    }
                }
            };
            let help_style = match &self.fetch_state {
                FetchState::Fetching => Style::default().fg(Color::Yellow),
                _ => Style::default().fg(Color::DarkGray),
            };
            frame.render_widget(Paragraph::new(help_text).style(help_style), *help_area);
        }
    }
}
