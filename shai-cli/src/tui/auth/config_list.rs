use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Style},
    symbols::border,
    text::{Line, Span, Text},
    widgets::{Block, Borders, Padding, Paragraph, Wrap},
    Frame,
};
use shai_core::config::config::{ProviderConfig, ShaiConfig};
use std::collections::HashMap;
use std::io;
use url::Url;

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
        Self::from_load(None)
    }

    pub fn new_with_error(error: String) -> Self {
        Self::from_load(Some(error))
    }

    fn from_config(config: ShaiConfig, error_message: Option<String>) -> Self {
        Self {
            config,
            selected_index: 0,
            error_message,
        }
    }

    fn from_load(error: Option<String>) -> Self {
        let (config, load_error) = ShaiConfig::load_or_default();
        Self::from_config(config, error.or(load_error))
    }

    fn total_items(&self) -> usize {
        self.config.providers.len() + 1 // +1 for "Add provider" option
    }

    fn is_add_provider_selected(&self) -> bool {
        self.selected_index == self.config.providers.len()
    }
}

/// Host (and non-default port) of an http(s) base URL. Returns `None` for anything
/// that does not parse cleanly as an http(s) URL, so the raw value is never shown.
fn url_host(raw: &str) -> Option<String> {
    let url = Url::parse(raw.trim()).ok()?;
    if !matches!(url.scheme(), "http" | "https") {
        return None;
    }
    // An '@' that did not end up in the userinfo means credentials were split at a
    // '/', '?' or '#' inside them, so the "host" may be part of a secret.
    if raw.contains('@') && url.username().is_empty() && url.password().is_none() {
        return None;
    }
    let host = url.host_str().filter(|host| !host.is_empty())?;
    Some(match url.port() {
        Some(port) => format!("{host}:{port}"),
        None => host.to_string(),
    })
}

/// "<provider> - <model>", plus " @ <host>" when the config has a usable `*_BASE_URL`.
fn base_label(provider_config: &ProviderConfig) -> String {
    let mut base_urls: Vec<_> = provider_config
        .env_vars
        .iter()
        .filter(|(name, _)| name.ends_with("_BASE_URL"))
        .collect();
    base_urls.sort();
    let host = base_urls.into_iter().find_map(|(_, url)| url_host(url));

    match host {
        Some(host) => format!(
            "{} - {} @ {}",
            provider_config.provider, provider_config.model, host
        ),
        None => format!("{} - {}", provider_config.provider, provider_config.model),
    }
}

/// Whether an API key is configured. Only this boolean is ever displayed, never
/// anything derived from the key itself.
fn has_api_key(provider_config: &ProviderConfig) -> bool {
    provider_config
        .env_vars
        .iter()
        .any(|(name, value)| name.ends_with("_API_KEY") && !value.trim().is_empty())
}

/// List labels for all configured providers. Labels that would otherwise be
/// identical get "(api key)" / "(anonymous)", and a 1-based position if that still
/// does not tell them apart.
fn provider_labels(providers: &[ProviderConfig]) -> Vec<String> {
    fn counts(labels: &[String]) -> HashMap<&str, usize> {
        let mut counts = HashMap::new();
        for label in labels {
            *counts.entry(label.as_str()).or_insert(0) += 1;
        }
        counts
    }

    let base: Vec<String> = providers.iter().map(base_label).collect();
    let base_counts = counts(&base);
    let marked: Vec<String> = base
        .iter()
        .zip(providers)
        .map(|(label, provider_config)| {
            if base_counts[label.as_str()] > 1 {
                let marker = if has_api_key(provider_config) {
                    "api key"
                } else {
                    "anonymous"
                };
                format!("{label} ({marker})")
            } else {
                label.clone()
            }
        })
        .collect();
    let marked_counts = counts(&marked);
    marked
        .iter()
        .enumerate()
        .map(|(i, label)| {
            if marked_counts[label.as_str()] > 1 {
                format!("{label} #{}", i + 1)
            } else {
                label.clone()
            }
        })
        .collect()
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
            KeyCode::Esc => NavAction::Done,
            KeyCode::Backspace | KeyCode::Char('d') => {
                // Delete the selected provider (only if it's not the "Add provider" option and we have providers)
                if !self.is_add_provider_selected() && !self.config.providers.is_empty() {
                    if let Err(e) = self.config.remove_provider(self.selected_index) {
                        self.error_message = Some(format!("Error removing provider: {}", e));
                        return NavAction::None;
                    }

                    // Adjust selected_index if needed
                    if self.selected_index >= self.total_items() {
                        self.selected_index = if self.total_items() > 0 {
                            self.total_items() - 1
                        } else {
                            0
                        };
                    }

                    // Save config after deletion
                    if let Err(e) = self.config.save() {
                        self.error_message =
                            Some(format!("Error saving config after deletion: {}", e));
                    }
                }
                NavAction::None
            }
            _ => NavAction::None,
        }
    }

    pub fn draw(&self, frame: &mut Frame, area: Rect) {
        let mut constraints = vec![Constraint::Length((4 + 1 + self.total_items() + 1) as u16)];

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
                top: 1,
                bottom: 1,
            })
            .title(" Select a Providers ")
            .style(Style::default().fg(Color::DarkGray));

        let mut lines = vec![];
        let labels = provider_labels(&self.config.providers);

        // Show existing providers
        for (i, label) in labels.iter().enumerate() {
            let prefix = if i == self.selected_index {
                "● "
            } else {
                "○ "
            };
            let selected_indicator = if i == self.config.selected_provider {
                " (current)"
            } else {
                ""
            };
            let line = format!("{}{}{}", prefix, label, selected_indicator);

            if i == self.selected_index {
                lines.push(Line::from(vec![Span::styled(
                    line,
                    Style::default().fg(Color::Green),
                )]));
            } else if i == self.config.selected_provider {
                lines.push(Line::from(vec![Span::styled(
                    line,
                    Style::default().fg(Color::Cyan),
                )]));
            } else {
                lines.push(Line::from(vec![Span::styled(
                    line,
                    Style::default().fg(Color::DarkGray),
                )]));
            }
        }

        // Add empty line separator
        lines.push(Line::from(""));

        // Add the "Add provider" option
        let add_line = "+ Add new provider";

        if self.is_add_provider_selected() {
            lines.push(Line::from(vec![Span::styled(
                add_line,
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(ratatui::style::Modifier::BOLD),
            )]));
        } else {
            lines.push(Line::from(vec![Span::styled(
                add_line,
                Style::default().fg(Color::DarkGray),
            )]));
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

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn key_press(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[tokio::test]
    async fn removing_last_provider_sets_error_message() {
        // ShaiConfig::default() ships with a single provider
        let mut modal = ModalConfig::from_config(ShaiConfig::default(), None);

        let action = modal.handle_event(key_press(KeyCode::Char('d'))).await;

        assert!(matches!(action, NavAction::None));
        assert_eq!(modal.config.providers.len(), 1);
        assert_eq!(
            modal.error_message.as_deref(),
            Some("Error removing provider: Cannot remove the last provider")
        );
    }

    #[tokio::test]
    async fn error_message_is_cleared_on_next_key_press() {
        let mut modal = ModalConfig::from_config(ShaiConfig::default(), Some("boom".to_string()));
        assert_eq!(modal.error_message.as_deref(), Some("boom"));

        modal.handle_event(key_press(KeyCode::Down)).await;

        assert_eq!(modal.error_message, None);
    }

    #[tokio::test]
    async fn add_provider_selection_returns_next_without_error() {
        let mut modal = ModalConfig::from_config(ShaiConfig::default(), None);
        // Move selection to the "+ Add new provider" entry
        modal.handle_event(key_press(KeyCode::Down)).await;

        let action = modal.handle_event(key_press(KeyCode::Enter)).await;

        assert!(matches!(action, NavAction::Next));
        assert_eq!(modal.error_message, None);
    }

    fn provider(name: &str, model: &str, env: &[(&str, &str)]) -> ProviderConfig {
        ProviderConfig {
            provider: name.to_string(),
            env_vars: env
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            model: model.to_string(),
            tool_method: shai_llm::ToolCallMethod::FunctionCall,
        }
    }

    const SECRET: &str = "sk-very-secret-key";

    fn render_list(config: ShaiConfig) -> String {
        use ratatui::{backend::TestBackend, Terminal};

        let modal = ModalConfig::from_config(config, None);
        let mut terminal = Terminal::new(TestBackend::new(100, 20)).unwrap();
        terminal
            .draw(|frame| modal.draw(frame, frame.area()))
            .unwrap();
        let buffer = terminal.backend().buffer();
        (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn url_host_shows_only_http_host_and_port() {
        assert_eq!(
            url_host("https://oai.endpoints.kepler.ai.cloud.ovh.net/v1").as_deref(),
            Some("oai.endpoints.kepler.ai.cloud.ovh.net")
        );
        assert_eq!(
            url_host("http://localhost:11434/v1").as_deref(),
            Some("localhost:11434")
        );
        assert_eq!(
            url_host("https://example.com:443/v1").as_deref(),
            Some("example.com")
        );
        assert_eq!(
            url_host("https://user:secret@proxy.example.com/v1?key=abc").as_deref(),
            Some("proxy.example.com")
        );
        assert_eq!(url_host("ftp://files.example.com/"), None);
        assert_eq!(url_host("localhost:11434"), None);
        assert_eq!(url_host(""), None);
    }

    #[test]
    fn url_host_never_leaks_passwords_with_delimiters() {
        for raw in [
            "https://user:pa/ss@proxy.example.com/v1",
            "https://user:12/ss@proxy.example.com/v1",
            "https://user:pa?ss@proxy.example.com/v1",
            "https://user:12?ss@proxy.example.com/v1",
            "https://user:pa#ss@proxy.example.com/v1",
            "https://user:12#ss@proxy.example.com/v1",
        ] {
            let host = url_host(raw);
            assert!(
                host.is_none() || host.as_deref() == Some("proxy.example.com"),
                "{raw} -> {host:?}"
            );
            let host = host.unwrap_or_default();
            assert!(!host.contains("user") && !host.contains("ss"), "{raw}");
        }
    }

    #[test]
    fn scheme_less_raw_key_in_base_url_is_not_shown() {
        let misconfigured = provider("ovhcloud", "Qwen3-32B", &[("OVH_BASE_URL", SECRET)]);
        assert_eq!(url_host(SECRET), None);
        assert_eq!(base_label(&misconfigured), "ovhcloud - Qwen3-32B");
    }

    #[test]
    fn labels_show_base_url_host_and_no_key_text() {
        let providers = vec![
            provider(
                "ovhcloud",
                "Qwen3-32B",
                &[(
                    "OVH_BASE_URL",
                    "https://qwen-3-32b.endpoints.kepler.ai.cloud.ovh.net/api/openai_compat/v1",
                )],
            ),
            provider(
                "ovhcloud",
                "Qwen3-32B",
                &[
                    ("OVH_API_KEY", SECRET),
                    (
                        "OVH_BASE_URL",
                        "https://oai.endpoints.kepler.ai.cloud.ovh.net/v1",
                    ),
                ],
            ),
            provider("openai", "gpt-4o", &[("OPENAI_API_KEY", SECRET)]),
        ];
        let labels = provider_labels(&providers);
        assert_eq!(
            labels,
            vec![
                "ovhcloud - Qwen3-32B @ qwen-3-32b.endpoints.kepler.ai.cloud.ovh.net",
                "ovhcloud - Qwen3-32B @ oai.endpoints.kepler.ai.cloud.ovh.net",
                "openai - gpt-4o",
            ]
        );
        for label in &labels {
            assert!(
                !label.contains("sk-") && !label.contains("secret"),
                "{label}"
            );
        }
    }

    #[test]
    fn anonymous_and_keyed_ovh_rows_render_differently() {
        // Added through `shai auth` with the base URL left blank: same provider,
        // same model, no host to tell them apart.
        let config = ShaiConfig {
            providers: vec![
                provider("ovhcloud", "Qwen3-32B", &[]),
                provider("ovhcloud", "Qwen3-32B", &[("OVH_API_KEY", SECRET)]),
            ],
            ..ShaiConfig::default()
        };
        let labels = provider_labels(&config.providers);
        assert_eq!(
            labels,
            vec![
                "ovhcloud - Qwen3-32B (anonymous)",
                "ovhcloud - Qwen3-32B (api key)",
            ]
        );

        let screen = render_list(config);
        assert!(
            screen.contains("ovhcloud - Qwen3-32B (anonymous)"),
            "{screen}"
        );
        assert!(
            screen.contains("ovhcloud - Qwen3-32B (api key)"),
            "{screen}"
        );
        assert!(
            !screen.contains("sk-") && !screen.contains("secret"),
            "{screen}"
        );
    }

    #[test]
    fn same_host_duplicates_get_marker_then_position() {
        let url = (
            "OVH_BASE_URL",
            "https://oai.endpoints.kepler.ai.cloud.ovh.net/v1",
        );
        let providers = vec![
            provider("ovhcloud", "Qwen3-32B", &[url]),
            provider("ovhcloud", "Qwen3-32B", &[url, ("OVH_API_KEY", SECRET)]),
            provider("ovhcloud", "Qwen3-32B", &[url, ("OVH_API_KEY", "sk-other")]),
            provider("mistral", "codestral", &[("MISTRAL_API_KEY", SECRET)]),
        ];
        let host = "ovhcloud - Qwen3-32B @ oai.endpoints.kepler.ai.cloud.ovh.net";
        assert_eq!(
            provider_labels(&providers),
            vec![
                format!("{host} (anonymous)"),
                format!("{host} (api key) #2"),
                format!("{host} (api key) #3"),
                "mistral - codestral".to_string(),
            ]
        );
    }

    #[test]
    fn multiline_error_is_fully_rendered() {
        use ratatui::{backend::TestBackend, Terminal};

        let modal = ModalConfig::from_config(
            ShaiConfig::default(),
            Some("Error: first line\nsecond line\nthird line".to_string()),
        );
        let backend = TestBackend::new(60, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| modal.draw(frame, frame.area()))
            .unwrap();

        let buffer = terminal.backend().buffer();
        let area = buffer.area;
        let text = (0..area.height)
            .map(|y| {
                (0..area.width)
                    .map(|x| buffer.get(x, y).symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");

        assert!(text.contains("first line"), "rendered text was:\n{text}");
        assert!(text.contains("second line"), "rendered text was:\n{text}");
        assert!(text.contains("third line"), "rendered text was:\n{text}");
    }
}
