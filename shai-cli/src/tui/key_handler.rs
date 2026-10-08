use std::io;
use std::time::Duration;

use ansi_to_tui::IntoText;
use arboard::Clipboard;
use crossterm::event::{Event, KeyCode, KeyEventKind, MouseEvent, MouseEventKind};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::widgets::Clear;

use super::input::{AgentMode, UserAction};
use super::session_picker::SessionPicker;
use super::ui_state::AppModalState;
use super::viewer::AlternateScreenViewer;
use super::App;

impl App<'_> {
    pub(crate) async fn handle_crossterm_event(&mut self, event: Event) -> io::Result<()> {
        match event {
            Event::Resize(..) => {
                if let Some(ref mut terminal) = self.terminal {
                    terminal.clear()?;
                }
            }
            Event::Mouse(mouse_event) => match mouse_event.kind {
                MouseEventKind::ScrollUp => {
                    self.renderer.history_mut().scroll_up(3);
                }
                MouseEventKind::ScrollDown => {
                    self.renderer.history_mut().scroll_down(3);
                }
                _ => {}
            },
            Event::Key(key_event) if key_event.kind == KeyEventKind::Press => {
                let ctrl = key_event
                    .modifiers
                    .contains(crossterm::event::KeyModifiers::CONTROL);
                match key_event.code {
                    KeyCode::PageUp => {
                        self.renderer.history_mut().scroll_up(3);
                        return Ok(());
                    }
                    KeyCode::PageDown => {
                        self.renderer.history_mut().scroll_down(3);
                        return Ok(());
                    }
                    KeyCode::Up if ctrl => {
                        self.renderer.history_mut().scroll_up(3);
                        return Ok(());
                    }
                    KeyCode::Down if ctrl => {
                        self.renderer.history_mut().scroll_down(3);
                        return Ok(());
                    }
                    _ => {}
                }
                self.renderer.history_mut().scroll_to_bottom();
                self.handle_key_event(key_event).await?;
            }
            _ => {}
        }
        Ok(())
    }

    async fn handle_session_picker_key(&mut self, key_event: crossterm::event::KeyEvent) -> io::Result<()> {
        use super::session_picker::SessionPickerAction;

        let picker = self.ui_state.session_picker.as_mut().unwrap();
        match picker.handle_key_event(key_event) {
            Some(SessionPickerAction::Selected(idx)) => {
                let sessions = shai_core::session::SessionPersist::list_sessions();
                self.ui_state.session_picker = None;
                match sessions {
                    Ok(sessions) if idx < sessions.len() => {
                        let session = &sessions[idx];
                        let session_id = session.session_id.clone();
                        let trace = session.trace.clone();

                        self.input.alert_msg(
                            &format!("Restoring session {}...", &session_id[..8]),
                            Duration::from_secs(2),
                        );

                        if let Some(agent) = self.agent.take() {
                            let _ = agent.controller.terminate().await;
                        }

                        let agent_name = self.agent_meta.name().map(|s| s.to_string());
                        self.start_agent(agent_name.as_deref())
                            .await
                            .map_err(|e| {
                                io::Error::other(format!("Failed to start agent: {}", e))
                            })?;

                        if let Some(ref agent) = self.agent {
                            let _ = agent.controller.load_trace(trace.clone()).await;
                        }

                        self.agent_state
                            .session_manager_mut()
                            .set_session_id(&session_id);
                        self.render_restored_trace(&trace);

                        self.input.alert_msg(
                            &format!("Session {} restored", &session_id[..8]),
                            Duration::from_secs(2),
                        );
                    }
                    _ => {
                        self.input
                            .alert_msg("Failed to restore session", Duration::from_secs(2));
                    }
                }
            }
            Some(SessionPickerAction::Cancelled) => {
                self.ui_state.session_picker = None;
            }
            None => {}
        }
        Ok(())
    }

    async fn handle_agent_picker_key(&mut self, key_event: crossterm::event::KeyEvent) -> io::Result<()> {
        use super::agent_picker::AgentPickerAction;

        let picker = self.ui_state.agent_picker.as_mut().unwrap();
        match picker.handle_key_event(key_event) {
            Some(AgentPickerAction::Selected(name)) => {
                self.ui_state.agent_picker = None;
                self.input.alert_msg(
                    &format!("Switching to agent '{}'...", name),
                    Duration::from_secs(2),
                );
                self.swap_agent(Some(&name)).await.map_err(|e| {
                    io::Error::other(format!("Failed to switch agent: {}", e))
                })?;
                self.input.alert_msg(
                    &format!("Switched to agent '{}'", name),
                    Duration::from_secs(2),
                );
            }
            Some(AgentPickerAction::Cancelled) => {
                self.ui_state.agent_picker = None;
            }
            None => {}
        }
        Ok(())
    }

    pub(crate) async fn handle_key_event(&mut self, key_event: crossterm::event::KeyEvent) -> io::Result<()> {
        if self.shortcuts.matches(&key_event, self.shortcuts.exit()) {
            self.ui_state.exit = true;
            return Ok(());
        }

        if self.ui_state.session_picker.is_some() {
            return self.handle_session_picker_key(key_event).await;
        }

        if self.ui_state.agent_picker.is_some() {
            return self.handle_agent_picker_key(key_event).await;
        }

        if self.shortcuts.matches(&key_event, self.shortcuts.toggle_theme()) {
            self.status_bar.theme_mut().toggle();
            let new_palette = self.status_bar.palette();
            self.input.set_palette(new_palette);
            return Ok(());
        }

        if self.shortcuts.matches(&key_event, self.shortcuts.clear_screen()) {
            if let Some(ref mut terminal) = self.terminal {
                terminal.clear()?;
            }
            self.renderer.history_mut().clear();
            return Ok(());
        }

        if self.shortcuts.matches(&key_event, self.shortcuts.regenerate()) {
            if let Some(ref agent) = self.agent {
                let _ = agent.controller.regenerate().await;
                self.notify("Regenerating last response...", Duration::from_secs(2));
            }
            return Ok(());
        }

        if self.shortcuts.matches(&key_event, self.shortcuts.copy_response()) {
            let last_response = self.agent_state.session_manager().last_assistant_response();
            if !last_response.is_empty() {
                if let Ok(mut ctx) = Clipboard::new() {
                    let _ = ctx.set_text(last_response.to_string());
                    self.notify("Copied last response to clipboard", Duration::from_secs(2));
                }
            } else {
                self.notify("No assistant response to copy", Duration::from_secs(2));
            }
            return Ok(());
        }

        if self.shortcuts.matches(&key_event, self.shortcuts.cycle_agent_mode()) {
            let mode = self.input.cycle_agent_mode();
            self.status_bar.set_agent_mode(&mode.status_bar_str());
            if let Some(ref agent) = self.agent {
                match mode {
                    AgentMode::Plan => {
                        let _ = agent.controller.plan_mode().await;
                        let _ = agent.controller.no_sudo().await;
                    }
                    AgentMode::Manual => {
                        let _ = agent.controller.no_plan_mode().await;
                        let _ = agent.controller.no_sudo().await;
                    }
                    AgentMode::Auto => {
                        let _ = agent.controller.no_plan_mode().await;
                        let _ = agent.controller.sudo().await;
                    }
                }
            }
            return Ok(());
        }

        if self.shortcuts.matches(&key_event, self.shortcuts.expand_tool()) {
            if let Some(output) = self
                .agent_state
                .tool_tracker()
                .last_output()
                .map(|s| s.to_string())
            {
                let file_path = self
                    .agent_state
                    .tool_tracker()
                    .last_file_path()
                    .map(|s| s.to_string());
                let mut viewer = AlternateScreenViewer::new(output, file_path);
                let _ = viewer.run().await;
            }
            return Ok(());
        }

        if key_event.code == KeyCode::Char('r')
            && key_event.modifiers.contains(crossterm::event::KeyModifiers::ALT)
        {
            let raw_text = self.renderer.history_mut().raw_text();
            if raw_text.trim().is_empty() {
                self.notify("No history to display", Duration::from_secs(2));
                return Ok(());
            }
            let _ = crossterm::execute!(std::io::stdout(), crossterm::event::DisableMouseCapture);
            let mut viewer = AlternateScreenViewer::new(raw_text, None);
            let _ = viewer.run().await;
            let _ = crossterm::execute!(std::io::stdout(), crossterm::event::EnableMouseCapture);
            return Ok(());
        }

        if self.shortcuts.matches(&key_event, self.shortcuts.session_picker()) {
            let sessions = shai_core::session::SessionPersist::list_sessions().unwrap_or_default();
            self.ui_state.session_picker =
                Some(SessionPicker::new(sessions, self.status_bar.palette()));
            return Ok(());
        }

        if self.shortcuts.matches(&key_event, self.shortcuts.prompt_picker()) {
            let prompts = shai_core::tools::prompts::discover_prompts();
            let active = shai_core::tools::prompts::load_active_prompts_from_disk();
            let mut picker = crate::tui::prompt_picker::PromptPicker::new(
                prompts,
                &active,
                self.status_bar.palette(),
            );
            match picker.run().await {
                Ok(crate::tui::prompt_picker::PromptPickerAction::Selected(selected)) => {
                    if let Some(ref agent) = self.agent {
                        let _ = agent.controller.set_active_prompts(selected.clone()).await;
                        let _ = shai_core::tools::prompts::save_active_prompts(&selected);
                        self.notify("System prompts updated", std::time::Duration::from_secs(2));
                    }
                }
                _ => {}
            }
            return Ok(());
        }

        match &mut self.ui_state.modal_state {
            AppModalState::InputShown => {
                let action = self.input.handle_event(key_event).await;
                self.handle_user_action(action).await?;
            }
            AppModalState::PermissionModal { widget } => {
                let action = widget.handle_key_event(key_event);
                self.handle_permission_action(action).await?;
            }
        }
        Ok(())
    }

    pub(crate) async fn handle_permission_action(
        &mut self,
        action: super::perm::PermissionModalAction,
    ) -> io::Result<()> {
        match action {
            super::perm::PermissionModalAction::Response { request_id, choice } => {
                if let Some(ref agent) = self.agent {
                    if matches!(
                        choice,
                        shai_core::agent::events::PermissionResponse::AllowAlways
                    ) {
                        let _ = agent.controller.sudo().await;
                        self.input.set_agent_mode(AgentMode::Auto);
                        self.status_bar.set_agent_mode(&AgentMode::Auto.status_bar_str());
                    }
                    if let Err(_) = agent
                        .controller
                        .response_permission_request(request_id, choice)
                        .await
                    {
                        self.notify(
                            "channel with agent closed. Please restart the app",
                            Duration::from_secs(3),
                        );
                    }
                }

                self.agent_state.permission_manager_mut().pop();
                self.ui_state.modal_state = AppModalState::InputShown;
            }
            super::perm::PermissionModalAction::Nope => {}
        }
        Ok(())
    }

    pub(crate) async fn check_permission_queue(&mut self) -> io::Result<()> {
        use super::perm::PermissionWidget;

        match &self.ui_state.modal_state {
            AppModalState::InputShown if !self.agent_state.permission_manager().is_empty() => {
                let (request_id, request) = self.agent_state.permission_manager().front().unwrap();
                let palette = self.status_bar.palette();
                let widget = PermissionWidget::new(
                    request_id.clone(),
                    request.clone(),
                    self.agent_state.permission_manager().len(),
                    palette,
                );

                let terminal_height = self
                    .terminal
                    .as_ref()
                    .and_then(|t| t.size().ok())
                    .map(|s| s.height)
                    .unwrap_or(24);

                if widget.height() > terminal_height.saturating_sub(5) {
                    let action =
                        match super::perm_alt_screen::AlternateScreenPermissionModal::new(&widget, palette) {
                            Ok(mut modal) => modal.run().await.unwrap_or_else(|_| {
                                super::perm::PermissionModalAction::Response {
                                    request_id: request_id.clone(),
                                    choice: shai_core::agent::PermissionResponse::Deny,
                                }
                            }),
                            Err(_) => super::perm::PermissionModalAction::Response {
                                request_id: request_id.clone(),
                                choice: shai_core::agent::PermissionResponse::Deny,
                            },
                        };
                    self.handle_permission_action(action).await?;
                } else {
                    self.ui_state.modal_state =
                        AppModalState::PermissionModal { widget };
                }
            }
            AppModalState::PermissionModal { .. }
                if self.agent_state.permission_manager().is_empty() =>
            {
                self.ui_state.modal_state = AppModalState::InputShown;
            }
            _ => {}
        }
        Ok(())
    }

    pub(crate) async fn handle_user_action(&mut self, action: UserAction) -> io::Result<()> {
        match action {
            UserAction::Nope => {}
            UserAction::CancelTask => {
                if let Some(ref agent) = self.agent {
                    let _ = agent.controller.stop_current_task().await;
                    self.notify("Task cancelled", Duration::from_secs(1));
                }
            }
            UserAction::UserInput { input } => {
                if let Some(ref agent) = self.agent {
                    if agent.controller.send_user_input(input.clone()).await.is_err() {
                        self.notify(
                            "channel with agent closed. Please restart the app",
                            Duration::from_secs(3),
                        );
                    }
                }
            }
            UserAction::UserAppCommand { command } => {
                let _ = super::command::CommandRegistry::dispatch(&command, self).await;
            }
        }
        Ok(())
    }

    pub(crate) fn draw_ui(&mut self) -> io::Result<()> {
        let modal_height = match &self.ui_state.modal_state {
            AppModalState::InputShown => self.input.height(),
            AppModalState::PermissionModal { widget } => widget.height(),
        }
        .max(5);

        let running_tools_height = self.agent_state.tool_tracker().len() as u16;

        if let Some(ref mut terminal) = self.terminal {
            terminal.draw(|frame| {
                let [_, history_area, _, tools_area, modal_area, statusbar_area] =
                    Layout::vertical([
                        Constraint::Length(1),
                        Constraint::Fill(1),
                        Constraint::Length(2),
                        Constraint::Length(running_tools_height),
                        Constraint::Length(modal_height),
                        Constraint::Length(1),
                    ])
                    .areas(frame.area());

                self.renderer.history_mut().draw(frame, history_area);

                if !self.agent_state.tool_tracker().is_empty() {
                    let layout: std::rc::Rc<[Rect]> =
                        Layout::vertical(vec![Constraint::Length(1); self.agent_state.tool_tracker().len()])
                            .split(tools_area);
                    for ((tool_id, tc), &area) in self
                        .agent_state
                        .tool_tracker()
                        .iter()
                        .zip(&*layout)
                    {
                        let elapsed = self.agent_state.tool_tracker().elapsed(tool_id).unwrap_or_default();
                        let secs = elapsed.as_secs();
                        let millis = elapsed.subsec_millis() / 10;
                        let tool_str = self.renderer.formatter().format_tool_running(tc);
                        let tool_with_time =
                            format!("{} ({:.1}s)", tool_str, secs as f64 + millis as f64 / 100.0);
                        frame.render_widget(tool_with_time.into_text().unwrap(), area);
                    }
                }

                match &self.ui_state.modal_state {
                    AppModalState::InputShown => self.input.draw(frame, modal_area),
                    AppModalState::PermissionModal { widget } => widget.draw(frame, modal_area),
                }

                self.status_bar.draw(frame, statusbar_area);

                if let Some(ref mut picker) = self.ui_state.session_picker {
                    let picker_area = Rect {
                        x: 2,
                        y: 1,
                        width: frame.area().width.saturating_sub(4),
                        height: frame.area().height.saturating_sub(2),
                    };
                    frame.render_widget(Clear, picker_area);
                    picker.draw(frame, picker_area);
                }

                if let Some(ref mut picker) = self.ui_state.agent_picker {
                    let picker_area = Rect {
                        x: 2,
                        y: 1,
                        width: frame.area().width.saturating_sub(4),
                        height: frame.area().height.saturating_sub(2),
                    };
                    frame.render_widget(Clear, picker_area);
                    picker.draw(frame, picker_area);
                }
            })?;
        }

        Ok(())
    }
}
