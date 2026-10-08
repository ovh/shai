#![allow(clippy::collapsible_if)]
#![allow(clippy::large_enum_variant)]
#![allow(clippy::collapsible_match)]
use std::io::{self, Write};

use crossterm::execute;
use crossterm::terminal::{self, disable_raw_mode};
use futures::StreamExt;
use ratatui::prelude::CrosstermBackend;
use ratatui::Terminal;
use ratatui::{TerminalOptions, Viewport};
use shai_core::agent::{AgentController, AgentEvent, PublicAgentState};
use shai_core::runners::coder::coder::coder;
use std::sync::Arc;
use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use tokio::time::{interval, Duration};
use tracing::{debug, warn};

use super::agent_meta::AgentMeta;
use super::agent_state::AgentState;

use super::handler::AgentHandler;
use super::input::InputArea;
use super::renderer::RenderManager;
use super::session_picker::SessionPicker;
use super::shortcuts::Shortcuts;
use super::statusbar::StatusBar;
use super::theme::Theme;
use super::ui_state::UiState;

pub struct AppRunningAgent {
    pub(crate) handle: JoinHandle<()>,
    pub(crate) events: broadcast::Receiver<AgentEvent>,
    pub(crate) controller: AgentController,
    pub(crate) tools: Vec<(String, String)>,
}

pub enum InitialModal {
    None,
    AgentPicker,
    SessionPicker,
}

pub struct App<'a> {
    pub(crate) terminal: Option<Terminal<CrosstermBackend<io::Stdout>>>,
    pub(crate) agent: Option<AppRunningAgent>,
    pub(crate) agent_state: AgentState,
    pub(crate) agent_meta: AgentMeta,
    pub(crate) ui_state: UiState<'a>,
    pub(crate) renderer: RenderManager,
    pub(crate) input: InputArea<'a>,
    pub(crate) shortcuts: Shortcuts,
    pub(crate) status_bar: StatusBar,
    pub(crate) initial_modal: InitialModal,
    pub(crate) initial_prompt: Option<String>,
}

impl App<'_> {
    pub async fn start_agent(
        &mut self,
        agent_name: Option<&str>,
    ) -> Result<String, Box<dyn std::error::Error>> {
        use shai_core::agent::builder::AgentBuilder;
        use shai_core::config::agent::AgentConfig;
        use shai_core::config::config::ShaiConfig;

        let mut agent: Box<dyn shai_core::agent::Agent> = if let Some(name) = agent_name {
            let config = AgentConfig::load(name)?;
            self.agent_meta.set_model(config.llm_provider.model.clone());
            self.agent_meta
                .set_provider(config.llm_provider.provider.clone());
            self.agent_meta.set_name(Some(name.to_string()));
            let agent_builder = AgentBuilder::from_config(config).await?;
            self.agent_state
                .mcp_manager_mut()
                .set_servers(agent_builder.mcp_status.clone());
            Box::new(agent_builder.build())
        } else {
            let (llm, model) = ShaiConfig::get_llm().await?;
            self.agent_meta.set_model(model.clone());
            self.agent_meta
                .set_provider(llm.provider().name().to_string());
            self.agent_meta.set_name(None);
            Box::new(coder(Arc::new(llm), model))
        };

        let banner = if let Some(name) = agent_name {
            format!(
                "\x1b[2m░ agent {} - {} on {}\x1b[0m",
                name,
                self.agent_meta.model(),
                self.agent_meta.provider()
            )
        } else {
            format!(
                "\x1b[2m░ {} on {}\x1b[0m",
                self.agent_meta.model(),
                self.agent_meta.provider()
            )
        };

        let controller = agent.controller();
        let events = agent.watch();
        let handle = tokio::spawn(async move {
            match agent.run().await {
                Ok(result) => debug!(target: "agent::loop", "Agent completed: {:?}", result),
                Err(error) => warn!(target: "agent::loop", "Agent failed: {}", error),
            }
        });

        self.agent = Some(AppRunningAgent {
            handle,
            controller: controller.clone(),
            events,
            tools: Vec::new(),
        });

        let saved_prompts = shai_core::tools::prompts::load_active_prompts_from_disk();
        if !saved_prompts.is_empty() {
            let _ = controller.set_active_prompts(saved_prompts).await;
        }

        self.status_bar.set_model(self.agent_meta.model());
        self.status_bar.set_provider(self.agent_meta.provider());
        self.refresh_status_bar();

        Ok(banner)
    }

    async fn receive_agent_event(&mut self) -> Option<AgentEvent> {
        if let Some(ref mut agent) = self.agent {
            agent.events.recv().await.ok()
        } else {
            None
        }
    }

    pub(crate) fn render_restored_trace(
        &mut self,
        trace: &[openai_dive::v1::resources::chat::ChatMessage],
    ) {
        use openai_dive::v1::resources::chat::{ChatMessage, ChatMessageContent};

        for message in trace {
            let formatted = match message {
                ChatMessage::User { content, .. } => match content {
                    ChatMessageContent::Text(text) => self
                        .renderer
                        .formatter()
                        .format_event(&AgentEvent::UserInput {
                            input: text.clone(),
                        }),
                    _ => None,
                },
                ChatMessage::Assistant { content, .. } => {
                    if let Some(ChatMessageContent::Text(text)) = content {
                        if text.trim().is_empty() {
                            None
                        } else {
                            Some(format!("\n\u{25cf} {}", text))
                        }
                    } else {
                        None
                    }
                }
                _ => None,
            };
            if let Some(text) = formatted {
                self.renderer.history_mut().add_text(&text);
            }
        }
    }

    pub(crate) async fn restore_session(&mut self, session_id: &str) -> io::Result<()> {
        use openai_dive::v1::resources::chat::{ChatMessage, ChatMessageContent};

        let session =
            shai_core::session::SessionPersist::load_session(session_id).map_err(|e| {
                io::Error::other(format!("Failed to load session {}: {}", session_id, e))
            })?;

        self.agent_state
            .session_manager_mut()
            .set_session_id(&session.session_id);

        if let Some(ref agent) = self.agent {
            let _ = agent.controller.load_trace(session.trace.clone()).await;
        }

        let user_inputs: Vec<String> = session
            .trace
            .iter()
            .filter_map(|msg| match msg {
                ChatMessage::User {
                    content: ChatMessageContent::Text(text),
                    ..
                } => Some(text.clone()),
                _ => None,
            })
            .collect();
        self.input.set_history(user_inputs);

        self.render_restored_trace(&session.trace);
        self.renderer.history_mut().scroll_to_bottom();
        self.refresh_status_bar();
        Ok(())
    }

    pub async fn swap_agent(
        &mut self,
        agent_name: Option<&str>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let trace = if let Some(ref agent) = self.agent {
            agent.controller.get_trace().await.ok()
        } else {
            None
        };

        if let Some(agent) = self.agent.take() {
            let _ = agent.controller.terminate().await;
            let _ = agent.handle.await;
        }

        self.start_agent(agent_name).await?;

        if let Some(trace) = trace {
            if let Some(ref agent) = self.agent {
                let _ = agent.controller.load_trace(trace).await;
            }
        }

        Ok(())
    }

    async fn handle_agent_event(&mut self, event: AgentEvent) -> io::Result<()> {
        self.agent_state.handle_event(&event).await;
        self.renderer.handle_event(&event).await;

        if let AgentEvent::StatusChanged { new_status, .. } = &event {
            self.input
                .set_agent_running(!matches!(new_status, PublicAgentState::Paused));
            self.status_bar
                .set_agent_mode(&self.input.agent_mode().status_bar_str());
            if matches!(new_status, PublicAgentState::Paused) {
                if let Some(ref agent_ref) = self.agent {
                    if let Ok(trace) = agent_ref.controller.get_trace().await {
                        let sid = self.agent_state.session_manager().session_id().to_string();
                        tokio::spawn(async move {
                            if let Err(e) =
                                shai_core::session::SessionPersist::save_session(&sid, trace)
                            {
                                tracing::warn!("Failed to save session {}: {}", sid, e);
                            }
                        });
                    }
                }
            }
        }

        if let AgentEvent::TokenUsage { .. } = &event {
            self.status_bar.set_tokens(
                self.agent_state.token_counter().input_tokens(),
                self.agent_state.token_counter().output_tokens(),
            );
        }

        self.refresh_status_bar();
        Ok(())
    }

    fn refresh_status_bar(&mut self) {
        if let Ok(cwd) = std::env::current_dir() {
            self.status_bar
                .set_location(&cwd.to_string_lossy().to_string());
        }
        if let Ok(output) = std::process::Command::new("git")
            .args(["rev-parse", "--abbrev-ref", "HEAD"])
            .output()
        {
            if output.status.success() {
                let branch = String::from_utf8_lossy(&output.stdout);
                self.status_bar.set_git_branch(branch.trim());
            }
        }
        self.status_bar
            .set_tool_call_method(self.input.tool_call_method_str());
    }
}

// UI-related Internals
impl App<'_> {
    pub fn new() -> Self {
        let theme = Theme::from_env();
        let palette = theme.palette();
        let shortcuts = Shortcuts::load();
        let mut input = InputArea::new(palette);
        input.set_shortcuts(
            shortcuts.cancel_task().clone(),
            shortcuts.clear_input().clone(),
            shortcuts.paste().clone(),
        );

        Self {
            terminal: None,
            agent: None,
            agent_state: AgentState::new(),
            agent_meta: AgentMeta::new(),
            ui_state: UiState::new(),
            renderer: RenderManager::new(),
            input,
            shortcuts,
            status_bar: StatusBar::new(theme),
            initial_modal: InitialModal::None,
            initial_prompt: None,
        }
    }

    pub fn notify(&mut self, msg: &str, duration: std::time::Duration) {
        self.status_bar.set_notification(msg, duration);
    }

    pub async fn run(
        &mut self,
        agent_name: Option<String>,
        restore_session_id: Option<String>,
    ) -> io::Result<()> {
        let x = self.try_run(agent_name, restore_session_id).await;

        let _ = execute!(
            std::io::stdout(),
            crossterm::event::DisableMouseCapture,
            crossterm::event::PopKeyboardEnhancementFlags,
        );
        std::io::stdout().flush().ok();
        let _ = disable_raw_mode();

        if let Err(e) = x {
            println!();
            eprintln!("{}\r\n", e);
        }

        println!();
        println!();
        Ok(())
    }

    async fn try_run(
        &mut self,
        agent_name: Option<String>,
        restore_session_id: Option<String>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let agent_name_ref = agent_name.as_deref();
        let banner =
            self.start_agent(agent_name_ref)
                .await
                .map_err(|e| -> Box<dyn std::error::Error> {
                    if let Some(name) = agent_name_ref {
                        format!("could not start custom agent '{}': {}", name, e).into()
                    } else {
                        "could not start shai agent, run shai auth first"
                            .to_string()
                            .into()
                    }
                })?;

        self.terminal = Some(ratatui::init_with_options(TerminalOptions {
            viewport: Viewport::Fullscreen,
        }));

        execute!(std::io::stdout(), crossterm::event::EnableMouseCapture)?;

        if let Some(ref mut terminal) = self.terminal {
            terminal.clear()?;
        }

        if !banner.is_empty() {
            self.renderer.history_mut().add_text(&banner);
        }

        std::io::stdout().flush().ok();

        if let Some(session_id) = restore_session_id {
            self.restore_session(&session_id).await?;
        }

        // Show initial modal if requested
        match self.initial_modal {
            InitialModal::AgentPicker => {
                self.ui_state.agent_picker =
                    Some(super::agent_picker::AgentPicker::new(self.status_bar.palette()));
            }
            InitialModal::SessionPicker => {
                let sessions =
                    shai_core::session::SessionPersist::list_sessions().unwrap_or_default();
                self.ui_state.session_picker =
                    Some(SessionPicker::new(sessions, self.status_bar.palette()));
            }
            InitialModal::None => {}
        }

        // Send initial prompt if provided (interactive mode)
        if let Some(prompt) = self.initial_prompt.take() {
            if let Some(ref agent) = self.agent {
                let _ = agent.controller.send_user_input(prompt).await;
            }
        }

        let mut animation_timer = interval(Duration::from_millis(100));
        let mut reader = crossterm::event::EventStream::new();

        while !self.ui_state.exit {
            self.draw_ui()
                .map_err(|_| -> Box<dyn std::error::Error> { "oops... (x_x)'".into() })?;

            tokio::select! {
                agent_event = self.receive_agent_event(), if self.agent.is_some() => {
                    if let Some(event) = agent_event {
                        self.handle_agent_event(event).await?;
                    }
                }

                crossterm_event = reader.next() => {
                    if let Some(Ok(event)) = crossterm_event {
                        self.handle_crossterm_event(event).await?;
                    }
                }

                _ = animation_timer.tick() => {
                    if let Some(action) = self.input.check_pending_enter() {
                        self.handle_user_action(action).await?;
                    }
                }
            }

            self.check_permission_queue().await?;
        }
        Ok(())
    }
}
