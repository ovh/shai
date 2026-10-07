use ratatui::layout::Rect;
use shai_core::agent::events::AgentEvent;
use shai_core::agent::output::PrettyFormatter;

use super::handler::AgentHandler;
use super::history::ConversationHistory;

pub struct RenderManager {
    history: ConversationHistory,
    formatter: PrettyFormatter,
}

impl RenderManager {
    pub fn new() -> Self {
        let skin = shai_core::config::tui::TuiConfig::load().markdown_skin();
        Self {
            history: ConversationHistory::new(),
            formatter: PrettyFormatter::with_theme(skin),
        }
    }

    #[allow(dead_code)] // used by tests
    pub fn history(&self) -> &ConversationHistory {
        &self.history
    }

    pub fn history_mut(&mut self) -> &mut ConversationHistory {
        &mut self.history
    }

    pub fn formatter(&self) -> &PrettyFormatter {
        &self.formatter
    }
}

#[async_trait::async_trait]
impl AgentHandler for RenderManager {
    async fn handle_event(&mut self, event: &AgentEvent) {
        let stick = self.history.at_bottom();
        let formatted = self.formatter.format_event(event).or_else(|| {
            // Fallback only if the formatter has no representation for the event
            match event {
                AgentEvent::Error { error } => {
                    Some(format!("\x1b[31m\u{2718} Error: {}\x1b[0m", error))
                }
                _ => None,
            }
        });
        if let Some(text) = formatted {
            self.history.add_text(&text);
            if stick {
                self.history.scroll_to_bottom();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_handle_error_event() {
        let mut renderer = RenderManager::new();
        let event = AgentEvent::Error {
            error: "test error".to_string(),
        };
        renderer.handle_event(&event).await;
        assert!(renderer.history().at_bottom());
    }

    #[tokio::test]
    async fn test_error_event_rendered_only_once() {
        let mut renderer = RenderManager::new();
        let event = AgentEvent::Error {
            error: "test error".to_string(),
        };
        renderer.handle_event(&event).await;
        let raw = renderer.history().raw_text();
        assert_eq!(raw.matches("test error").count(), 1);
    }

    #[tokio::test]
    async fn test_handle_completed_event() {
        let mut renderer = RenderManager::new();
        let event = AgentEvent::Completed {
            success: true,
            message: "done".to_string(),
        };
        renderer.handle_event(&event).await;
        assert!(renderer.history().at_bottom());
    }

    #[tokio::test]
    async fn test_sticky_scroll_follows_when_at_bottom() {
        let mut renderer = RenderManager::new();
        renderer.history_mut().add_text(&"line\n".repeat(50));
        assert!(renderer.history().at_bottom());

        let event = AgentEvent::Completed {
            success: true,
            message: "done".to_string(),
        };
        renderer.handle_event(&event).await;
        assert!(renderer.history().at_bottom());
    }

    #[tokio::test]
    async fn test_sticky_scroll_preserved_when_scrolled_up() {
        let mut renderer = RenderManager::new();
        renderer.history_mut().add_text(&"line\n".repeat(50));
        renderer.history_mut().scroll_up(10);
        let offset = renderer.history().scroll_offset();
        assert!(offset > 0);

        let event = AgentEvent::Completed {
            success: true,
            message: "done".to_string(),
        };
        renderer.handle_event(&event).await;
        assert_eq!(renderer.history().scroll_offset(), offset);
    }
}
