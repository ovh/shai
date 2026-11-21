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
use url::{Host, Url};

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
/// that does not parse cleanly as an http(s) URL to a plausible endpoint, so the raw
/// value (which may be a pasted key) is never shown.
fn url_host(raw: &str) -> Option<String> {
    let raw = raw.trim();
    let url = Url::parse(raw).ok()?;
    if !matches!(url.scheme(), "http" | "https") {
        return None;
    }
    // Any '@' outside the userinfo means credentials were split at a '/', '?' or
    // '#' inside them, so the "host" may be part of a secret. That holds whether or
    // not the parser found a (partial) userinfo before the split.
    if raw.contains('@') {
        let after_scheme = raw.split_once("://").map_or(raw, |(_, rest)| rest);
        let authority_end = after_scheme
            .find(['/', '?', '#', '\\'])
            .unwrap_or(after_scheme.len());
        if after_scheme[authority_end..].contains('@') {
            return None;
        }
    }
    let host = match url.host()? {
        Host::Ipv4(_) | Host::Ipv6(_) => url.host_str()?.to_string(),
        Host::Domain(domain) if is_plausible_domain(domain) => domain.to_string(),
        Host::Domain(_) => return None,
    };
    Some(match url.port() {
        Some(port) => format!("{host}:{port}"),
        None => host,
    })
}

/// `localhost`, or a dotted hostname none of whose labels looks like a secret.
/// Single-label names are rejected: `https://sk-...` parses with the key as host.
fn is_plausible_domain(domain: &str) -> bool {
    if domain.eq_ignore_ascii_case("localhost") {
        return true;
    }
    let labels: Vec<&str> = domain.trim_end_matches('.').split('.').collect();
    labels.len() >= 2
        && labels
            .iter()
            .all(|label| !label.is_empty() && !is_key_like(label))
}

/// Common API-key prefixes, or a long unbroken run of mixed letters and digits.
fn is_key_like(label: &str) -> bool {
    const KEY_PREFIXES: [&str; 6] = ["sk-", "sk_", "pk-", "pk_", "rk-", "key-"];
    let label = label.to_ascii_lowercase();
    if KEY_PREFIXES.iter().any(|prefix| label.starts_with(prefix)) {
        return true;
    }
    label.split(['-', '_']).any(|part| {
        part.len() >= 20
            && part.chars().any(|c| c.is_ascii_digit())
            && part.chars().any(|c| c.is_ascii_alphabetic())
    })
}

/// One provider row: `head` ("<provider> - <model>"), the optional base URL host,
/// and `tail` (the duplicate markers). Only the host is shortened to fit.
#[derive(Debug, Clone, PartialEq)]
struct ProviderLabel {
    head: String,
    host: Option<String>,
    tail: String,
}

impl ProviderLabel {
    fn full(&self) -> String {
        self.fit(usize::MAX)
    }

    /// The label in at most `width` columns where possible: the host is cut (with
    /// "…") or dropped before the head or the markers lose any text.
    fn fit(&self, width: usize) -> String {
        self.fit_shortened(width, None)
    }

    /// Like [`Self::fit`], but when `suffix_start` is set the host is shortened
    /// with a middle ellipsis that keeps `host[suffix_start..]`.
    fn fit_shortened(&self, width: usize, suffix_start: Option<usize>) -> String {
        let cols = |s: &str| Span::raw(s).width();
        let mut out = self.head.clone();
        if let Some(host) = &self.host {
            const SEP: &str = " @ ";
            let room = width.saturating_sub(cols(&self.head) + cols(&self.tail) + cols(SEP));
            if cols(host) <= room {
                out.push_str(SEP);
                out.push_str(host);
            } else if room >= 4 {
                // Hosts are ASCII (the URL parser punycodes them), one column per char.
                out.push_str(SEP);
                out.push_str(&shorten_host(host, room, suffix_start));
            }
        }
        out.push_str(&self.tail);
        out
    }
}

/// Cut `host` to `room` columns. Prefers a middle ellipsis that keeps the
/// distinguishing tail when that tail fits; otherwise a leading cut.
fn shorten_host(host: &str, room: usize, suffix_start: Option<usize>) -> String {
    if let Some(suffix_start) = suffix_start {
        if let Some(short) = shorten_middle(host, room, suffix_start) {
            return short;
        }
    }
    let mut short: String = host.chars().take(room - 1).collect();
    short.push('…');
    short
}

/// `prefix…tail` keeping `host[suffix_start..]`, if that tail fits in `room`.
fn shorten_middle(host: &str, room: usize, suffix_start: usize) -> Option<String> {
    let n = host.len();
    if n <= room {
        return Some(host.to_string());
    }
    let suffix_start = suffix_start.min(n);
    let suffix_len = n - suffix_start;
    let inner = room - 1;
    if suffix_len == 0 || suffix_len > inner {
        return None;
    }
    let prefix_len = inner - suffix_len;
    Some(format!("{}…{}", &host[..prefix_len], &host[suffix_start..]))
}

/// "<provider> - <model>", plus the host when the config has a usable `*_BASE_URL`.
fn base_label(provider_config: &ProviderConfig) -> ProviderLabel {
    let mut base_urls: Vec<_> = provider_config
        .env_vars
        .iter()
        .filter(|(name, _)| name.ends_with("_BASE_URL"))
        .collect();
    base_urls.sort();
    ProviderLabel {
        head: format!("{} - {}", provider_config.provider, provider_config.model),
        host: base_urls.into_iter().find_map(|(_, url)| url_host(url)),
        tail: String::new(),
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

/// Labels for all configured providers. Labels that would otherwise be identical
/// get "(api key)" / "(anonymous)", and a 1-based position if that still does not
/// tell them apart.
fn provider_labels(providers: &[ProviderConfig]) -> Vec<ProviderLabel> {
    fn counts(labels: &[String]) -> HashMap<&str, usize> {
        let mut counts = HashMap::new();
        for label in labels {
            *counts.entry(label.as_str()).or_insert(0) += 1;
        }
        counts
    }

    let mut labels: Vec<ProviderLabel> = providers.iter().map(base_label).collect();
    let base: Vec<String> = labels.iter().map(ProviderLabel::full).collect();
    let base_counts = counts(&base);
    for ((label, key), provider_config) in labels.iter_mut().zip(&base).zip(providers) {
        if base_counts[key.as_str()] > 1 {
            let marker = if has_api_key(provider_config) {
                "api key"
            } else {
                "anonymous"
            };
            label.tail = format!(" ({marker})");
        }
    }
    let marked: Vec<String> = labels.iter().map(ProviderLabel::full).collect();
    let marked_counts = counts(&marked);
    for (i, (label, key)) in labels.iter_mut().zip(&marked).enumerate() {
        if marked_counts[key.as_str()] > 1 {
            label.tail.push_str(&format!(" #{}", i + 1));
        }
    }
    labels
}

/// First index at which not all hosts share the same character, or `None`
/// when they are identical.
fn first_differing_index(hosts: &[&str]) -> Option<usize> {
    if hosts.len() < 2 {
        return None;
    }
    let min_len = hosts.iter().map(|h| h.len()).min()?;
    for i in 0..min_len {
        let b = hosts[0].as_bytes()[i];
        if hosts.iter().any(|h| h.as_bytes()[i] != b) {
            return Some(i);
        }
    }
    if hosts.iter().any(|h| h.len() != min_len) {
        Some(min_len)
    } else {
        None
    }
}

fn collision_groups(fitted: &[String]) -> Vec<Vec<usize>> {
    let mut groups: HashMap<&str, Vec<usize>> = HashMap::new();
    for (i, s) in fitted.iter().enumerate() {
        groups.entry(s.as_str()).or_default().push(i);
    }
    groups
        .into_values()
        .filter(|group| group.len() > 1)
        .collect()
}

/// Fit each label to its row width, then make any collisions distinguishable.
/// Prefer a middle ellipsis that keeps the differing host tail; if that still
/// collides (or the tail does not fit), add a 1-based position marker, reserved
/// within the width so existing markers stay visible.
fn fit_distinct(labels: &[ProviderLabel], rooms: &[usize]) -> Vec<String> {
    let mut fitted: Vec<String> = labels
        .iter()
        .zip(rooms)
        .map(|(label, &room)| label.fit(room))
        .collect();

    for group in collision_groups(&fitted) {
        let hosts: Vec<&str> = group
            .iter()
            .filter_map(|&i| labels[i].host.as_deref())
            .collect();
        if let Some(suffix_start) = first_differing_index(&hosts) {
            for &i in &group {
                fitted[i] = labels[i].fit_shortened(rooms[i], Some(suffix_start));
            }
        }
    }

    for group in collision_groups(&fitted) {
        for i in group {
            let mut label = labels[i].clone();
            label.tail.push_str(&format!(" #{}", i + 1));
            fitted[i] = label.fit(rooms[i]);
        }
    }

    fitted
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
        // Inside the rounded border and the 1-column padding on each side.
        let label_width = layout_areas[0].width.saturating_sub(4) as usize;
        let rooms: Vec<usize> = (0..labels.len())
            .map(|i| {
                let prefix = 2; // "● " / "○ "
                let current = if i == self.config.selected_provider {
                    Span::raw(" (current)").width()
                } else {
                    0
                };
                label_width.saturating_sub(prefix + current)
            })
            .collect();
        let fitted = fit_distinct(&labels, &rooms);
        for (i, fitted_label) in fitted.iter().enumerate() {
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
            let line = format!("{}{}{}", prefix, fitted_label, selected_indicator);

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

    fn full_labels(providers: &[ProviderConfig]) -> Vec<String> {
        provider_labels(providers)
            .iter()
            .map(ProviderLabel::full)
            .collect()
    }

    fn render_list(config: ShaiConfig) -> String {
        render_list_sized(config, 100)
    }

    fn render_list_sized(config: ShaiConfig, width: u16) -> String {
        use ratatui::{backend::TestBackend, Terminal};

        let modal = ModalConfig::from_config(config, None);
        let mut terminal = Terminal::new(TestBackend::new(width, 20)).unwrap();
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
    fn url_host_rejects_at_after_authority_even_with_userinfo() {
        for raw in [
            "https://user:pa@secret-fragment/rest@proxy.example.com/v1",
            "https://user:pa@secret.fragment/rest@proxy.example.com/v1",
            "https://user@secret.fragment?rest@proxy.example.com/v1",
            "https://user:pa@secret.fragment#rest@proxy.example.com/v1",
            "https://proxy.example.com/v1?key=a@b",
        ] {
            assert_eq!(url_host(raw), None, "{raw}");
        }
        // A well-formed userinfo still shows only the host.
        assert_eq!(
            url_host("https://user:pass@proxy.example.com:8443/v1").as_deref(),
            Some("proxy.example.com:8443")
        );
    }

    #[test]
    fn url_host_rejects_key_like_or_single_label_hosts() {
        for raw in [
            "https://sk-very-secret-key",
            "https://sk-very-secret-key/v1",
            "http://ollama:11434/v1",
            "https://sk-proj-abc.example.com/v1",
            "https://a1b2c3d4e5f6g7h8i9j0k1l2.example.com/v1",
        ] {
            assert_eq!(url_host(raw), None, "{raw}");
        }
        for (raw, host) in [
            ("http://127.0.0.1:11434/v1", "127.0.0.1:11434"),
            ("http://[::1]:11434/v1", "[::1]:11434"),
            ("http://LOCALHOST:11434/v1", "localhost:11434"),
            ("https://api.openai.com/v1", "api.openai.com"),
            (
                "https://deepseek-r1-distill-llama-70b.endpoints.kepler.ai.cloud.ovh.net/v1",
                "deepseek-r1-distill-llama-70b.endpoints.kepler.ai.cloud.ovh.net",
            ),
        ] {
            assert_eq!(url_host(raw).as_deref(), Some(host), "{raw}");
        }
    }

    #[test]
    fn narrow_list_disambiguates_truncated_hosts() {
        // Two non-current configs whose hosts share a long prefix: at width 40
        // a leading cut would render both as "○ ovhcloud - Qwen3-32B @ abcdefghij…".
        let config = ShaiConfig {
            providers: vec![
                provider("openai", "gpt-4o", &[]),
                provider(
                    "ovhcloud",
                    "Qwen3-32B",
                    &[("OVH_BASE_URL", "https://abcdefghija.example.com/v1")],
                ),
                provider(
                    "ovhcloud",
                    "Qwen3-32B",
                    &[("OVH_BASE_URL", "https://abcdefghijb.example.com/v1")],
                ),
            ],
            ..ShaiConfig::default()
        };
        let screen = render_list_sized(config, 40);
        let rows: Vec<&str> = screen.lines().filter(|l| l.contains("ovhcloud")).collect();
        assert_eq!(rows.len(), 2, "{screen}");
        assert!(rows[0].contains("#2"), "{screen}");
        assert!(rows[1].contains("#3"), "{screen}");
        assert_ne!(rows[0].trim_end(), rows[1].trim_end(), "{screen}");
        for row in &rows {
            assert!(row.contains("ovhcloud - Qwen3-32B @"), "{screen}");
        }
    }

    #[test]
    fn fit_distinct_leaves_non_colliding_labels_unchanged() {
        let labels = vec![
            ProviderLabel {
                head: "ovhcloud - Qwen3-32B".to_string(),
                host: Some("oai.endpoints.kepler.ai.cloud.ovh.net".to_string()),
                tail: String::new(),
            },
            ProviderLabel {
                head: "ovhcloud - Qwen3-32B".to_string(),
                host: Some("qwen-3-32b.endpoints.kepler.ai.cloud.ovh.net".to_string()),
                tail: String::new(),
            },
        ];
        let rooms = [34usize, 34];
        let fitted = fit_distinct(&labels, &rooms);
        assert_eq!(fitted[0], labels[0].fit(34));
        assert_eq!(fitted[1], labels[1].fit(34));
        assert_ne!(fitted[0], fitted[1]);
        assert!(!fitted[0].contains('#'), "{}", fitted[0]);
        assert!(!fitted[1].contains('#'), "{}", fitted[1]);
    }

    #[test]
    fn fit_distinct_keeps_distinguishing_host_tail_when_it_fits() {
        // Difference is near the end, so a leading cut still collides at this
        // width, but a middle ellipsis can keep the unique suffix.
        let labels = vec![
            ProviderLabel {
                head: "ovhcloud - Qwen3-32B".to_string(),
                host: Some("verylongsharedprefix.service-a.example.com".to_string()),
                tail: String::new(),
            },
            ProviderLabel {
                head: "ovhcloud - Qwen3-32B".to_string(),
                host: Some("verylongsharedprefix.service-b.example.com".to_string()),
                tail: String::new(),
            },
        ];
        // Host room = width - 23; 17 columns yields "ver…a.example.com".
        let width = 40;
        assert_eq!(labels[0].fit(width), labels[1].fit(width));
        let fitted = fit_distinct(&labels, &[width, width]);
        assert_eq!(fitted[0], "ovhcloud - Qwen3-32B @ ver…a.example.com");
        assert_eq!(fitted[1], "ovhcloud - Qwen3-32B @ ver…b.example.com");
    }

    #[test]
    fn narrow_list_keeps_markers_and_positions() {
        let url = (
            "OVH_BASE_URL",
            "https://oai.endpoints.kepler.ai.cloud.ovh.net/v1",
        );
        let config = ShaiConfig {
            providers: vec![
                provider("ovhcloud", "Qwen3-32B", &[url]),
                provider("ovhcloud", "Qwen3-32B", &[url, ("OVH_API_KEY", SECRET)]),
                provider("ovhcloud", "Qwen3-32B", &[url, ("OVH_API_KEY", "sk-other")]),
            ],
            ..ShaiConfig::default()
        };
        let screen = render_list_sized(config, 60);
        let rows: Vec<&str> = screen.lines().filter(|l| l.contains("ovhcloud")).collect();
        assert_eq!(rows.len(), 3, "{screen}");
        assert!(rows[0].contains("(anonymous) (current)"), "{screen}");
        assert!(rows[1].contains("(api key) #2"), "{screen}");
        assert!(rows[2].contains("(api key) #3"), "{screen}");
        for row in &rows {
            assert!(row.contains("ovhcloud - Qwen3-32B @ oai.end"), "{screen}");
            assert!(row.contains('…'), "host is shortened: {screen}");
        }
    }

    #[test]
    fn fit_shortens_only_the_host() {
        let label = ProviderLabel {
            head: "ovhcloud - Qwen3-32B".to_string(),
            host: Some("oai.endpoints.kepler.ai.cloud.ovh.net".to_string()),
            tail: " (api key) #2".to_string(),
        };
        assert_eq!(
            label.fit(100),
            "ovhcloud - Qwen3-32B @ oai.endpoints.kepler.ai.cloud.ovh.net (api key) #2"
        );
        let fitted = label.fit(45);
        assert_eq!(fitted, "ovhcloud - Qwen3-32B @ oai.endp… (api key) #2");
        assert_eq!(Span::raw(fitted.as_str()).width(), 45);
        // No room for a useful host: drop it, keep head and markers.
        assert_eq!(label.fit(20), "ovhcloud - Qwen3-32B (api key) #2");
    }

    #[test]
    fn scheme_less_raw_key_in_base_url_is_not_shown() {
        let misconfigured = provider("ovhcloud", "Qwen3-32B", &[("OVH_BASE_URL", SECRET)]);
        assert_eq!(url_host(SECRET), None);
        assert_eq!(base_label(&misconfigured).full(), "ovhcloud - Qwen3-32B");
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
        let labels = full_labels(&providers);
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
        let labels = full_labels(&config.providers);
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
            full_labels(&providers),
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
