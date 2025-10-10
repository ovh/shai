use std::io::{self, stdin, stdout};
use std::sync::Arc;
use std::time::Instant;

use chrono::Utc;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind};
use crossterm::terminal::{self, disable_raw_mode, enable_raw_mode};
use crossterm::{execute, cursor, ExecutableCommand};
use futures::{future::FutureExt, select, StreamExt};
use ratatui::layout::Rect;
use ratatui::prelude::CrosstermBackend;
use ratatui::style::Stylize;
use ratatui::text::{Line, Span, Text};
use ratatui::Terminal;
use shai_core::agent::{Agent, AgentRequest, AgentEvent, AgentController, PublicAgentState};
use shai_core::agent::events::{PermissionRequest, PermissionResponse};
use shai_core::agent::output::PrettyFormatter;
use shai_core::config::config::ShaiConfig;
use shai_core::config::agent::AgentConfig;
use shai_core::agent::builder::AgentBuilder;
use shai_core::logging::LoggingConfig;
use shai_core::runners::coder::coder::coder;
use shai_core::tools::{ToolCall, ToolResult};
use shai_llm::{LlmClient, ToolCallMethod};
use shai_llm::tool::max_context::get_max_context;
use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Color, Style},
    widgets::{Paragraph, Widget},
    Frame, TerminalOptions, Viewport
};
use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use tokio::time::{interval, Duration};
use tui_textarea::Input;
use ansi_to_tui::IntoText;
use std::collections::{HashMap, VecDeque};

use crate::tui::input::InputArea;
use super::input::UserAction;
use crate::tui::perm::PermissionWidget;
use crate::tui::perm_alt_screen::AlternateScreenPermissionModal;
use super::perm::PermissionModalAction;


pub enum AppModalState<'a> {
    InputShown,
    PermissionModal {
        widget: PermissionWidget<'a>   
    }
}

pub struct AppRunningAgent {
    pub(crate) handle:     JoinHandle<()>,
    pub(crate) events:     broadcast::Receiver<AgentEvent>,
    pub(crate) controller: AgentController,
}

pub struct App<'a> {
    pub(crate) terminal: Option<Terminal<CrosstermBackend<io::Stdout>>>,
    pub(crate) terminal_height: u16,

    pub(crate) agent: Option<AppRunningAgent>,
    pub(crate) custom_agent: Option<Box<dyn Agent>>,

    pub(crate) state: AppModalState<'a>,
    pub(crate) formatter: PrettyFormatter, // streaming log formatter
    pub(crate) running_tools: HashMap<String, ToolCall>, // (request_id, request)
    pub(crate) input: InputArea<'a>,       // input text
    pub(crate) commands: HashMap<(String, String),Vec<String>>,
    pub(crate) exit: bool,
    pub(crate) permission_queue: VecDeque<(String, PermissionRequest)>, // (request_id, request)

    pub(crate) total_input_tokens: u32,
    pub(crate) total_output_tokens: u32,
    pub(crate) current_tokens: usize,
    pub(crate) max_context: usize,
}


// Agent-related Internals
impl App<'_> {
    pub async fn start_agent(&mut self, agent_name: Option<&str>) -> Result<(), Box<dyn std::error::Error>> {
        let mut agent: Box<dyn Agent> = if let Some(agent_name) = agent_name {
            // Load custom agent config
            let config = AgentConfig::load(agent_name)?;
            
            println!("\x1b[2m░ agent {} - {} on {}\x1b[0m", agent_name, config.llm_provider.model, config.llm_provider.provider);
            self.max_context = get_max_context(&config.llm_provider.model);
            
            // Create agent from config
            let agent_builder = AgentBuilder::from_config(config).await?;
            Box::new(agent_builder.build())
        } else {
            // Use default coder agent
            let (llm, model) = ShaiConfig::get_llm().await?;
            println!("\x1b[2m░ {} on {}\x1b[0m", model, llm.provider().name());
            self.max_context = get_max_context(&model);
            Box::new(coder(Arc::new(llm), model))
        };
        
        // Get Agent I/O
        let controller = agent.controller();
        let events = agent.watch();

        // Run the agent in background
        let handle = tokio::spawn(async move {
            match agent.run().await {
                Ok(result) => eprintln!("Agent completed: {:?}", result),
                Err(error) => eprintln!("Agent failed: {:?}", error),
            }
        });

        self.agent = Some(AppRunningAgent{
            handle,
            controller,
            events
        });
        Ok(())
    }

    async fn receive_agent_event(&mut self) -> Option<AgentEvent> {
        if let Some(ref mut agent) = self.agent {
            agent.events.recv().await.ok()
        } else {
            None
        }
    }

    async fn handle_agent_event(&mut self, event: AgentEvent) -> io::Result<()> {
        // Update agent state
        if let AgentEvent::StatusChanged { new_status, .. } = &event {
            self.input.set_agent_running(!matches!(new_status, PublicAgentState::Paused));
        }

        // updated inprogress list
        if let AgentEvent::ToolCallStarted { call, .. }= &event {
            self.running_tools.insert(call.tool_call_id.clone(), call.clone());
        }
        if let AgentEvent::ToolCallCompleted { call, .. }= &event {
            self.running_tools.remove(&call.tool_call_id);
        }

        // Format and display event
        if let Some(formatted) = self.formatter.format_event(&event) {
            if let Some(ref mut terminal) = self.terminal {
                let wrapped = formatted.into_text().unwrap();
                let line_count = wrapped.lines.iter().len() as u16;
                terminal.clear()?; // this is to avoid visual artifact
                terminal.insert_before(line_count, |buf| {
                    wrapped.render(buf.area, buf);
                })?;
            }
        }

        // Handle permission requests - just add to queue
        if let AgentEvent::PermissionRequired { request_id, request } = &event {
            self.permission_queue.push_back((request_id.clone(), request.clone()));
        }

        // Handle token usage tracking
        if let AgentEvent::TokenUsage { input_tokens, output_tokens } = &event {
            self.total_input_tokens += input_tokens;
            self.total_output_tokens += output_tokens;
            self.current_tokens += (input_tokens + output_tokens) as usize;
        }
        // Update current_tokens after context compression
        if let AgentEvent::ContextCompressed { current_tokens, .. } = &event {
            if let Some(ct) = current_tokens {
                self.current_tokens = *ct as usize;
            }
        }
        
        Ok(())
    }
}


// UI-related Internals
impl App<'_> {
    pub fn new() -> Self {
        Self {
            terminal: None,
            terminal_height: 5,
            agent: None,
            custom_agent: None,
            formatter: PrettyFormatter::new(),
            state: AppModalState::InputShown,
            input: InputArea::new(),
            commands: Self::list_command(),
            exit: false,
            running_tools: HashMap::new(),
            permission_queue: VecDeque::new(),
            total_input_tokens: 0,
            total_output_tokens: 0,
            current_tokens: 0,
            max_context: 0,
        }
    }

    pub async fn run(&mut self, agent_name: Option<String>) -> io::Result<()> {
        let x = self.try_run(agent_name).await;
        let _ = disable_raw_mode();

        if let Err(e) = x {
            // Simply print a newline to move cursor to next line and beginning
            println!();
            eprintln!("{}\r\n", e);
        }

        println!();
        println!();
        Ok(())
    }

    async fn try_run(&mut self, agent_name: Option<String>) ->Result<(), Box<dyn std::error::Error>> {
        // Start the agent (custom or default)
        let agent_name_ref = agent_name.as_deref();
        self.start_agent(agent_name_ref).await.map_err(|e| -> Box<dyn std::error::Error> { 
            if agent_name_ref.is_some() {
                format!("could not start custom agent '{}': {}", agent_name_ref.unwrap(), e).into()
            } else {
                format!("could not start shai agent, run shai auth first").into()
            }
        })?;
        
        // create terminal
        self.terminal = Some(ratatui::init_with_options(TerminalOptions {
            viewport: Viewport::Inline(8)
        }));

        // Create a timer for animation updates
        let mut animation_timer = interval(Duration::from_millis(100));
        let mut reader = crossterm::event::EventStream::new();

        while !self.exit {
            // Always draw the UI first
            self.draw_ui().map_err(|_| -> Box<dyn std::error::Error> { 
                format!("oops... (x_x)'").into() })?;

            tokio::select! {
                // Handle agent events (only when not in permission modal)
                agent_event = self.receive_agent_event(), if self.agent.is_some() => {
                    if let Some(event) = agent_event {
                        self.handle_agent_event(event).await?;
                    }
                }
                
                // Handle keyboard input
                crossterm_event = reader.next() => {
                    if let Some(Ok(event)) = crossterm_event {
                        self.handle_crossterm_event(event).await?;
                    }
                }
                
                // Handle animation timer (fires when animating OR when checking for pending enter)
                _ = animation_timer.tick() => {
                    // Check for pending enter timeout
                    if let Some(action) = self.input.check_pending_enter() {
                        self.handle_user_action(action).await?;
                    }
                    // Timer ticked, UI will be redrawn in next iteration
                }
            }
            
            // Check permission queue and update state
            self.check_permission_queue().await?;
        }
        Ok(())
    }

    async fn handle_crossterm_event(&mut self, event: Event) -> io::Result<()> {
        match event {
            Event::Resize( .. ) => {
                if let Some(ref mut terminal) = self.terminal {
                    terminal.clear()?;
                }
            }
            Event::Key(key_event) if key_event.kind == KeyEventKind::Press => {
                self.handle_key_event(key_event).await?;
            }
            _ => {}
        }
        Ok(())
    }

    async fn handle_key_event(&mut self, key_event: KeyEvent) -> io::Result<()> {
        if (matches!(key_event.code, KeyCode::Char('c')) && key_event.modifiers.contains(crossterm::event::KeyModifiers::CONTROL)) || (matches!(key_event.code, KeyCode::Char('d')) && key_event.modifiers.contains(crossterm::event::KeyModifiers::CONTROL)) {
            self.exit = true;
            return Ok(());
        }

        match &mut self.state {
            AppModalState::InputShown => {
                let action = self.input.handle_event(key_event).await;
                self.handle_user_action(action).await?;
            },
            AppModalState::PermissionModal { widget  } => {
                let action = widget.handle_key_event(key_event).await;
                self.handle_permission_action(action).await?;
            }
        }
        Ok(())
    }


    async fn handle_permission_action(&mut self, action: PermissionModalAction) -> io::Result<()> {
        match action {
            PermissionModalAction::Response { request_id, choice } => {
                // Send response to agent
                if let Some(ref agent) = self.agent {     
                    if matches!(choice, PermissionResponse::AllowAlways) {
                        let _ = agent.controller.sudo().await;
                    }        
                    match agent.controller.response_permission_request(request_id, choice).await {
                        Err(e) => {
                            self.input.alert_msg("channel with agent closed. Please restart the app", Duration::from_secs(3));
                        },
                        _ => {},
                    }
                }
                
                // Remove the completed permission from queue
                self.permission_queue.pop_front();
                
                // Go back to InputShown so next check_permission_queue will show next permission
                self.state = AppModalState::InputShown;
            }
            PermissionModalAction::Nope => {}
        }
        Ok(())
    }

    async fn check_permission_queue(&mut self) -> io::Result<()> {
        match &self.state {
            AppModalState::InputShown if !self.permission_queue.is_empty() => {
                let (request_id, request) = self.permission_queue.front().unwrap();
                let widget = PermissionWidget::new(
                    request_id.clone(), 
                    request.clone(), 
                    self.permission_queue.len()
                );
                
                let terminal_height = self.terminal.as_ref()
                    .and_then(|t| t.size().ok())
                    .map(|s| s.height)
                    .unwrap_or(24);
                
                if widget.height() > terminal_height.saturating_sub(5) {
                    // Use alternate screen for large modals
                    if let Ok(mut modal) = AlternateScreenPermissionModal::new(&widget) {
                        let action = modal.run().await.unwrap_or(PermissionModalAction::Nope);
                        self.handle_permission_action(action).await?;
                    }
                } else {
                    // Use inline modal for small modals
                    self.state = AppModalState::PermissionModal { widget };
                }
            }
            AppModalState::PermissionModal { .. } if self.permission_queue.is_empty() => {
                self.state = AppModalState::InputShown;
            }
            _ => {}
        }
        Ok(())
    }

    async fn handle_user_action(&mut self, action: UserAction) -> io::Result<()> {
        match action {
            UserAction::Nope => {}
            UserAction::CancelTask => {
                if let Some(ref agent) = self.agent {
                    let _ = agent.controller.test_stop_current_task().await;
                    self.input.alert_msg("Task cancelled", Duration::from_secs(1));
                }
            }
            UserAction::UserInput { input } => {
                if let Some(ref agent) = self.agent {                                
                    match agent.controller.send_user_input(input.clone()).await {
                        Err(e) => {
                            self.input.alert_msg("channel with agent closed. Please restart the app", Duration::from_secs(3));
                        },
                        _ => {},
                    }
                }
            }
            UserAction::UserAppCommand { command } => {
                let _ = self.handle_app_command(&command).await;
            }
        }
        Ok(())
    }


    fn draw_ui(&mut self) -> io::Result<()> {
        let modal_height = match &self.state {
            AppModalState::InputShown => self.input.height(),
            AppModalState::PermissionModal { widget } => widget.height(),
        }.max(5);
        let height = modal_height
        + 2 
        + self.running_tools.len() as u16;

        if let Some(ref mut terminal) = self.terminal {  
            if height != self.terminal_height {
                terminal.set_viewport_height(height + 1)?;
                self.terminal_height = height;
            }

            terminal.draw(|frame| {                    
                let [_, inprogress, modal, ctx_line] = Layout::vertical([
                    Constraint::Length(1), // padding
                    Constraint::Length(self.running_tools.len() as u16 + 1), // running tool (if any)
                    Constraint::Length(modal_height),
                    Constraint::Length(1)
                ]).areas(frame.area());
            

                // draw running tool
                if !self.running_tools.is_empty() {
                    let layout: std::rc::Rc<[Rect]> = Layout::vertical(vec![Constraint::Length(1); self.running_tools.len()+1]).split(inprogress);
                    for ((_,tc), &area) in self.running_tools.iter().zip(layout.into_iter()) {
                        frame.render_widget(self.formatter.format_tool_running(tc).into_text().unwrap(), area);
                    }
                }

                // draw modal
                match &self.state {
                    AppModalState::InputShown => {
                        self.input.draw(frame, modal)
                    },
                    AppModalState::PermissionModal { widget } => {
                        widget.draw(frame, modal)
                    }
                }
                // Render context usage line
                if self.max_context > 0 {
                    let used = self.current_tokens;
                    let remaining = if used > self.max_context { 0 } else { self.max_context - used };
                    let percent = (remaining as f64 / self.max_context as f64) * 100.0;
                    let ctx_text = format!("Context: {:.1}% remaining", percent);
                    frame.render_widget(Span::styled(ctx_text, Style::default().fg(Color::DarkGray)), ctx_line);
                }
            })?;
        }
        Ok(())
    }

}

