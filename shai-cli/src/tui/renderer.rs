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
        Self {
            history: ConversationHistory::new(),
            formatter: PrettyFormatter::new(),
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
        if let Some(formatted) = self.formatter.format_event(event) {
            self.history.add_text(&formatted);
            self.history.scroll_to_bottom();
        }
        if let AgentEvent::Error { error } = event {
            let error_msg = format!("\x1b[31m\u{2718} Error: {}\x1b[0m", error);
            self.history.add_text(&error_msg);
            self.history.scroll_to_bottom();
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
    async fn test_handle_completed_event() {
        let mut renderer = RenderManager::new();
        let event = AgentEvent::Completed {
            success: true,
            message: "done".to_string(),
        };
        renderer.handle_event(&event).await;
        assert!(renderer.history().at_bottom());
    }
}
