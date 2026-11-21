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
use crate::session_options::SessionOptions;

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
    pub(crate) session_options: SessionOptions,
    /// Lines about the session's options that are waiting to be shown.
    pub(crate) session_notices: Vec<String>,
    /// Agents this session has started so far.
    pub(crate) agents_started: u32,
    pub(crate) event_reader: Option<crossterm::event::EventStream>,
}

impl App<'_> {
    pub async fn start_agent(
        &mut self,
        agent_name: Option<&str>,
    ) -> Result<String, Box<dyn std::error::Error>> {
        use shai_core::agent::builder::AgentBuilder;
        use shai_core::agent::Agent;
        use shai_core::config::agent::AgentConfig;

        // The agent this one replaces may have handed off without its
        // announcement having been read. Say so before moving on.
        self.show_unseen_switch();
        // What the session was asked for follows the config, which is read
        // again for every agent: a handoff still to come is aimed at what its
        // target resolves to now, or given up if it no longer resolves.
        if self.agents_started > 0 {
            let notices = self.session_options.refresh();
            self.session_notices.extend(notices);
        }

        let mut agent = if let Some(name) = agent_name {
            let config = AgentConfig::load(name)?;
            self.agent_meta.set_model(config.llm_provider.model.clone());
            self.agent_meta
                .set_provider(config.llm_provider.provider.clone());
            self.agent_meta.set_name(Some(name.to_string()));
            let agent_builder = AgentBuilder::from_config(config).await?;
            self.agent_state
                .mcp_manager_mut()
                .set_servers(agent_builder.mcp_status.clone());
            agent_builder.build()
        } else {
            // As at start-up: a config file that is there and cannot be read
            // is not replaced by the built-in default, which would send the
            // conversation to a provider the user did not choose.
            // The file is loaded once, and the primary model built from that
            // load, so it cannot break between a check and the read.
            let config = SessionOptions::primary_config()?;
            let (llm, model) = config.get_llm_from_config().await?;
            self.agent_meta.set_model(model.clone());
            self.agent_meta
                .set_provider(llm.provider().name().to_string());
            self.agent_meta.set_name(None);
            coder(Arc::new(llm), model)
        };

        // A handoff that has not happened yet stays armed on an agent that
        // replaces the previous one, if that agent can hand off at all. Once
        // it has happened, no agent is armed again.
        if let Some(notice) = self.session_options.arm(&mut agent) {
            self.session_notices.push(notice);
        }

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
        self.agents_started += 1;

        Ok(banner)
    }

    /// Show the lines about the session's options that are waiting.
    pub(crate) fn show_session_notices(&mut self) {
        if self.session_notices.is_empty() {
            return;
        }
        let stick = self.renderer.history().at_bottom();
        for notice in std::mem::take(&mut self.session_notices) {
            self.renderer.history_mut().add_system_text(&notice);
        }
        if stick {
            self.renderer.history_mut().scroll_to_bottom();
        }
    }

    /// Show a notice event, and follow a model switch in the display.
    fn show_notice(&mut self, event: &AgentEvent) {
        if let AgentEvent::ModelSwitched { provider, model } = event {
            // Show where the conversation goes from now on.
            self.agent_meta.set_model(model.clone());
            self.agent_meta.set_provider(provider.clone());
            self.status_bar.set_model(model);
            self.status_bar.set_provider(provider);
        }
        if let Some(notice) = shai_core::agent::notice_text(event) {
            let stick = self.renderer.history().at_bottom();
            self.renderer.history_mut().add_system_text(&notice);
            if stick {
                self.renderer.history_mut().scroll_to_bottom();
            }
            self.notify(&notice, Duration::from_secs(8));
        }
    }

    /// Announce a handoff that happened without its event reaching this
    /// loop: events are dropped when the loop falls far behind, and with an
    /// agent that is replaced before they are read.
    pub(crate) fn show_unseen_switch(&mut self) {
        if let Some(event) = self.session_options.take_unseen_switch() {
            self.show_notice(&event);
        }
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
                    ChatMessageContent::Text(text) => {
                        self.renderer
                            .formatter()
                            .format_event(&AgentEvent::UserInput {
                                input: text.clone(),
                            })
                    }
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

    /// Whether the running agent may be replaced by one on the agent
    /// `agent_name` (`None`: the primary model of the auth config). Not when
    /// that would start from an auth config that is there and cannot be
    /// read: the new agent would not start, and the session would be left
    /// without one. The user is told why.
    pub(crate) fn may_replace_agent(&mut self, agent_name: Option<&str>) -> bool {
        if agent_name.is_some() {
            return true;
        }
        self.may_replace_agent_given(SessionOptions::config_problem())
    }

    /// [`App::may_replace_agent`] for an agent on the auth config, given what
    /// is wrong with that config file, if anything.
    fn may_replace_agent_given(&mut self, config_problem: Option<String>) -> bool {
        match config_problem {
            Some(problem) => {
                self.session_notices
                    .push(format!("the agent was not replaced: {problem}"));
                false
            }
            None => true,
        }
    }

    pub async fn swap_agent(
        &mut self,
        agent_name: Option<&str>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if !self.may_replace_agent(agent_name) {
            return Ok(());
        }
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

        // One-shot: the handoff is over for this session. If it has been
        // announced already (see `show_unseen_switch`), this is a repeat.
        let repeat = match &event {
            AgentEvent::ModelSwitched { provider, model } => {
                !self.session_options.on_model_switched(provider, model)
            }
            _ => false,
        };
        if !repeat {
            self.show_notice(&event);
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
            self.status_bar.set_location(cwd.to_string_lossy().as_ref());
        }
        if self.status_bar.git_needs_refresh() {
            if let Ok(output) = std::process::Command::new("git")
                .args(["rev-parse", "--abbrev-ref", "HEAD"])
                .output()
            {
                if output.status.success() {
                    let branch = String::from_utf8_lossy(&output.stdout);
                    self.status_bar.set_git_branch(branch.trim());
                }
            }
        }
        self.status_bar
            .set_tool_call_method(self.input.tool_call_method_str());
    }
}

// UI-related Internals
impl App<'_> {
    pub fn new() -> Self {
        let theme = Theme::from_config();
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
            session_options: SessionOptions::default(),
            session_notices: Vec::new(),
            agents_started: 0,
            event_reader: None,
        }
    }

    /// Drop the main event reader so a nested full-screen view (auth,
    /// permission modal, viewer, ...) can create its own.
    ///
    /// Crossterm has a single global event reader behind a mutex, and an idle
    /// [`crossterm::event::EventStream`] keeps that mutex held in its waker
    /// thread while blocked on input. A nested view constructing a second
    /// stream would then deadlock until the next key press, leaving a black
    /// screen. Dropping the stream shuts its waker thread down and releases
    /// the lock; the global reader itself (and any queued events) survives.
    pub(crate) fn suspend_event_reader(&mut self) {
        self.event_reader = None;
    }

    /// Re-create the main event reader after a nested view exited.
    pub(crate) fn resume_event_reader(&mut self) {
        self.event_reader = Some(crossterm::event::EventStream::new());
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

        self.renderer
            .history_mut()
            .add_text(&super::theme::welcome_text());
        if !banner.is_empty() {
            self.renderer.history_mut().add_text(&banner);
        }
        // Lines about the session's options are shown by the loop below,
        // under a restored conversation rather than above it.

        std::io::stdout().flush().ok();

        if let Some(session_id) = restore_session_id {
            self.restore_session(&session_id).await?;
        }

        // Show initial modal if requested
        match self.initial_modal {
            InitialModal::AgentPicker => {
                self.ui_state.agent_picker = Some(super::agent_picker::AgentPicker::new(
                    self.status_bar.palette(),
                ));
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
        self.resume_event_reader();

        while !self.ui_state.exit {
            self.show_session_notices();
            self.draw_ui()
                .map_err(|_| -> Box<dyn std::error::Error> { "oops... (x_x)'".into() })?;

            let mut reader = self.event_reader.take().unwrap_or_default();

            tokio::select! {
                agent_event = self.receive_agent_event(), if self.agent.is_some() => {
                    self.event_reader = Some(reader);
                    match agent_event {
                        Some(event) => self.handle_agent_event(event).await?,
                        // Events were missed. A handoff may be among them.
                        None => self.show_unseen_switch(),
                    }
                    reader = self.event_reader.take().unwrap_or_default();
                }

                crossterm_event = reader.next() => {
                    self.event_reader = Some(reader);
                    if let Some(Ok(event)) = crossterm_event {
                        self.handle_crossterm_event(event).await?;
                    }
                    reader = self.event_reader.take().unwrap_or_default();
                }

                _ = animation_timer.tick() => {
                    self.event_reader = Some(reader);
                    if let Some(action) = self.input.check_pending_enter() {
                        self.handle_user_action(action).await?;
                    }
                    reader = self.event_reader.take().unwrap_or_default();
                }
            }

            self.event_reader = Some(reader);
            self.check_permission_queue().await?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session_options::test_support::hand_off;
    use shai_core::agent::Prewalk;
    use shai_core::config::roles::BoundModel;
    use shai_llm::LlmClient;

    /// A handoff to a target that is never contacted: building a client
    /// opens no socket.
    fn prewalk() -> Prewalk {
        Prewalk::new(BoundModel::new(
            Arc::new(LlmClient::ollama("http://127.0.0.1:9/v1".to_string())),
            "qwen3:8b",
        ))
    }

    fn switched() -> AgentEvent {
        AgentEvent::ModelSwitched {
            provider: "ollama".to_string(),
            model: "qwen3:8b".to_string(),
        }
    }

    fn app_with(prewalk: Prewalk) -> App<'static> {
        let mut app = App::new();
        app.agent_meta.set_model("big-model".to_string());
        app.agent_meta.set_provider("openai".to_string());
        app.session_options = SessionOptions::with_prewalk(prewalk);
        app
    }

    fn switch_lines(app: &App<'_>) -> usize {
        app.renderer
            .history()
            .raw_text()
            .matches("prewalk: switched to ollama/qwen3:8b")
            .count()
    }

    #[tokio::test]
    async fn a_model_switch_is_shown_and_ends_the_handoff() {
        let mut app = app_with(prewalk());
        assert!(app.session_options.prewalk_pending());

        app.handle_agent_event(switched()).await.unwrap();

        // The display follows the conversation to its new destination...
        assert_eq!(app.agent_meta.model(), "qwen3:8b");
        assert_eq!(app.agent_meta.provider(), "ollama");
        // ...the scrollback says so, once...
        assert_eq!(switch_lines(&app), 1);
        // ...and an agent started later in this session is not armed again.
        assert!(!app.session_options.prewalk_pending());

        // The same announcement arriving again is not shown twice.
        app.handle_agent_event(switched()).await.unwrap();
        assert_eq!(switch_lines(&app), 1);
    }

    #[tokio::test]
    async fn other_events_leave_the_handoff_armed() {
        let mut app = app_with(prewalk());

        app.handle_agent_event(AgentEvent::TokenUsage {
            input_tokens: 10,
            output_tokens: 5,
            cached_tokens: 0,
        })
        .await
        .unwrap();
        app.show_unseen_switch();

        assert_eq!(app.agent_meta.model(), "big-model");
        assert!(app.session_options.prewalk_pending());
        assert!(!app.renderer.history().raw_text().contains("prewalk"));
    }

    #[tokio::test]
    async fn a_switch_whose_event_was_lost_is_still_shown_once() {
        // An agent hands off, and its announcement never reaches the loop:
        // the loop fell behind, or the agent was replaced first.
        let handoff = prewalk();
        let mut app = app_with(handoff.clone());
        app.show_unseen_switch();
        assert_eq!(switch_lines(&app), 0);
        hand_off(&handoff, true).await;
        assert!(handoff.has_switched());

        app.show_unseen_switch();
        assert_eq!(switch_lines(&app), 1);
        assert_eq!(app.agent_meta.model(), "qwen3:8b");
        assert_eq!(app.agent_meta.provider(), "ollama");
        assert!(!app.session_options.prewalk_pending());

        // Asked again, or told by the event after all: nothing more to show.
        app.show_unseen_switch();
        app.handle_agent_event(switched()).await.unwrap();
        assert_eq!(switch_lines(&app), 1);
    }

    #[test]
    fn an_agent_is_not_replaced_by_one_that_could_not_start() {
        let mut app = App::new();
        assert!(app.may_replace_agent_given(None));
        assert!(app.session_notices.is_empty());
        assert!(!app.may_replace_agent_given(Some(
            "the config file /x/auth.config.json could not be loaded (line 3)".to_string()
        )));
        assert_eq!(
            app.session_notices,
            ["the agent was not replaced: the config file /x/auth.config.json could not be loaded (line 3)"]
        );
        // An agent of its own config does not depend on that file.
        assert!(app.may_replace_agent(Some("reviewer")));
    }

    #[test]
    fn waiting_session_notices_are_shown_and_cleared() {
        let mut app = App::new();
        app.show_session_notices();
        assert!(app.renderer.history().raw_text().is_empty());

        app.session_notices
            .push("prewalk not armed: this agent cannot change model".to_string());
        app.show_session_notices();
        app.show_session_notices();
        let scrollback = app.renderer.history().raw_text();
        assert_eq!(scrollback.matches("prewalk not armed").count(), 1);
        assert!(app.session_notices.is_empty());
    }
}
