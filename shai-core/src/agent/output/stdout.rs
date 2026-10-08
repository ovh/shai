use super::pretty::PrettyFormatter;
use crate::agent::{AgentEvent, AgentEventHandler};
use async_trait::async_trait;
use std::io::{self, Write};

/// Stdout event manager that formats and prints agent activity in a user-friendly way
pub struct StdoutEventManager {
    formatter: PrettyFormatter,
}

impl StdoutEventManager {
    pub fn new() -> Self {
        Self {
            formatter: PrettyFormatter::new(),
        }
    }
}

#[async_trait]
impl AgentEventHandler for StdoutEventManager {
    async fn handle_event(&self, event: AgentEvent) {
        if let Some(formatted) = self.formatter.format_event(&event) {
            eprintln!("{}", formatted);
            let _ = io::stdout().flush();
        }
    }
}

impl Default for StdoutEventManager {
    fn default() -> Self {
        Self::new()
    }
}
