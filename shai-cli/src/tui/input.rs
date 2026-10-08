use std::time::{Duration, Instant};

use arboard::Clipboard;
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use futures::io;
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Style, Stylize},
    symbols::border,
    text::Span,
    widgets::{Block, Borders, Padding, Widget},
    Frame,
};
use shai_llm::ToolCallMethod;
use ratatui_textarea::{Input as TextInput, TextArea};

use crate::tui::helper::HelpArea;

use super::suggestion::{CommandSuggestion, FileSuggestion};
use super::shortcuts::key_event_to_binding;
use super::theme::ThemePalette;
use shai_core::config::tui::KeyBinding;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentMode {
    Plan,
    Manual,
    Auto,
}

impl AgentMode {
    pub fn status_bar_str(&self) -> String {
        let symbol = match self {
            AgentMode::Plan => "\u{2612}",   // ☒
            AgentMode::Manual => "\u{2610}",  // ☐
            AgentMode::Auto => "\u{2611}",   // ☑
        };
        format!("{} {:?}", symbol, self)
    }
}

pub enum UserAction {
    Nope,
    CancelTask,
    UserInput { input: String },
    UserAppCommand { command: String },
}

pub struct InputArea<'a> {
    agent_running: bool,

    // input text
    input: TextArea<'a>,
    placeholder: String,

    // draft saving for history navigation
    current_draft: Option<String>,

    // alert top left
    animation_start: Option<Instant>,
    status_message: Option<String>,

    // status bottom left
    last_keystroke_time: Option<Instant>,
    pending_enter: Option<Instant>,
    helper_msg: Option<String>,
    helper_set: Option<Instant>,
    helper_duration: Option<Duration>,
    escape_press_time: Option<Instant>,

    // method info bottom right
    method: ToolCallMethod,
    agent_mode: AgentMode,

    // bottom helper
    help: Option<HelpArea>,

    history: Vec<String>,
    history_index: usize,

    // suggestions
    file_suggestion: FileSuggestion,
    cmd_suggestion: CommandSuggestion,

    // theme colors
    palette: ThemePalette,

    // configurable key bindings
    cancel_task_binding: KeyBinding,
    clear_input_binding: KeyBinding,
    paste_binding: KeyBinding,
}

impl InputArea<'_> {
    pub fn new(palette: ThemePalette) -> Self {
        Self {
            agent_running: false,
            input: TextArea::default(),
            placeholder: "? for help".to_string(),
            current_draft: None,
            animation_start: None,
            status_message: None,
            last_keystroke_time: None,
            pending_enter: None,
            helper_msg: None,
            helper_set: None,
            helper_duration: None,
            escape_press_time: None,
            method: ToolCallMethod::FunctionCall,
            agent_mode: AgentMode::Manual,
            help: None,
            history: Vec::new(),
            history_index: 0,
            file_suggestion: FileSuggestion::new(),
            cmd_suggestion: CommandSuggestion::new(),
            palette,
            // defaults — overridden by set_shortcuts()
            cancel_task_binding: KeyBinding::new(
                shai_core::config::tui::KeyCode::Escape,
                shai_core::config::tui::KeyModifiers::NONE,
            ),
            clear_input_binding: KeyBinding::new(
                shai_core::config::tui::KeyCode::Escape,
                shai_core::config::tui::KeyModifiers::NONE,
            ),
            paste_binding: KeyBinding::new(
                shai_core::config::tui::KeyCode::Char('v'),
                shai_core::config::tui::KeyModifiers::CONTROL,
            ),
        }
    }

    pub fn agent_mode(&self) -> AgentMode {
        self.agent_mode
    }

    pub fn set_agent_mode(&mut self, mode: AgentMode) {
        self.agent_mode = mode;
    }

    pub fn cycle_agent_mode(&mut self) -> AgentMode {
        self.agent_mode = match self.agent_mode {
            AgentMode::Plan => AgentMode::Manual,
            AgentMode::Manual => AgentMode::Auto,
            AgentMode::Auto => AgentMode::Plan,
        };
        self.agent_mode
    }

    pub fn set_shortcuts(
        &mut self,
        cancel_task: KeyBinding,
        clear_input: KeyBinding,
        paste: KeyBinding,
    ) {
        self.cancel_task_binding = cancel_task;
        self.clear_input_binding = clear_input;
        self.paste_binding = paste;
    }
}

/// alert message in yellow, top left
impl InputArea<'_> {
    pub fn set_history(&mut self, history: Vec<String>) {
        self.history = history;
        self.history_index = self.history.len();
    }

    pub fn set_palette(&mut self, palette: ThemePalette) {
        self.palette = palette;
    }

    pub fn alert_msg(&mut self, text: &str, duration: Duration) {
        self.helper_msg = Some(text.to_string());
        self.helper_set = Some(Instant::now());
        self.helper_duration = Some(duration);
    }

    pub fn set_agent_running(&mut self, running: bool) {
        self.agent_running = running;
        if running {
            self.animation_start = Some(Instant::now());
        } else {
            self.status_message = None;
            self.animation_start = None;
        }
    }

    fn get_status_text(&self) -> String {
        if let Some(ref msg) = self.status_message {
            format!(" {}", msg)
        } else if let Some(animation_start) = self.animation_start {
            let spinner_chars = [
                "\u{280B}", "\u{2819}", "\u{2839}", "\u{2838}", "\u{283C}", "\u{2834}", "\u{2826}",
                "\u{2827}", "\u{2825}", "\u{280F}",
            ];
            let elapsed = animation_start.elapsed().as_millis();
            let index = (elapsed / 100) % spinner_chars.len() as u128;
            format!(
                " {} Agent is working... (press esc to cancel)",
                spinner_chars[index as usize]
            )
        } else {
            String::new()
        }
    }
}

/// method info bottom right
impl InputArea<'_> {
    pub fn set_tool_call_method(&mut self, method: ToolCallMethod) {
        self.method = method;
    }

    pub fn tool_call_method_str(&self) -> &'static str {
        match self.method {
            ToolCallMethod::Auto => "",
            ToolCallMethod::FunctionCall => "",
            ToolCallMethod::FunctionCallRequired => "\u{2A10}fc2",
            ToolCallMethod::StructuredOutput => "\u{2A10}so",
            ToolCallMethod::Parsing => "\u{2A10}ps",
        }
    }
}

/// status message bottom left
impl InputArea<'_> {
    pub fn check_pending_enter(&mut self) -> Option<UserAction> {
        if let Some(enter_time) = self.pending_enter {
            if enter_time.elapsed() >= Duration::from_millis(100) {
                self.pending_enter = None;

                if self.agent_running {
                    return Some(UserAction::Nope);
                }

                let lines = self.input.lines();
                if !lines[0].is_empty() {
                    let input = lines.join("\n");
                    self.history.push(input.clone());
                    self.history_index = self.history.len();

                    self.input = TextArea::default();
                    if input.starts_with('/') {
                        return Some(UserAction::UserAppCommand { command: input });
                    } else {
                        return Some(UserAction::UserInput { input });
                    }
                }
            }
        }
        None
    }

    fn check_helper_msg(&mut self) -> String {
        if let Some(helper_time) = self.helper_set {
            if helper_time.elapsed() >= self.helper_duration.unwrap() {
                self.helper_msg = None;
                self.helper_set = None;
                self.helper_duration = None;
                return String::new();
            }
        }
        self.helper_msg.as_deref().unwrap_or("").to_string()
    }
}

/// event related
impl InputArea<'_> {
    fn move_cursor_to_end_of_text(&mut self) {
        for _ in 0..self.input.lines().len().saturating_sub(1) {
            self.input.move_cursor(ratatui_textarea::CursorMove::Down);
        }
        if let Some(last_line) = self.input.lines().last() {
            for _ in 0..last_line.len() {
                self.input.move_cursor(ratatui_textarea::CursorMove::Forward);
            }
        }
    }

    fn load_historic_prompt(&mut self, index: usize) {
        if let Some(entry) = self.history.get(index) {
            self.input = TextArea::new(entry.lines().map(|s| s.to_string()).collect());
            self.move_cursor_to_end_of_text();
        }
    }

    // Replace @search with the file path
    fn replace_file_search(&mut self, file_path: &str) {
        if let Some((at_pos, search_text)) = FileSuggestion::detect_file_search(&self.input) {
            let cursor = self.input.cursor();
            let _row = cursor.0;

            let chars_to_delete = 1 + search_text.len();

            self.input.move_cursor(ratatui_textarea::CursorMove::Head);
            for _ in 0..at_pos {
                self.input.move_cursor(ratatui_textarea::CursorMove::Forward);
            }

            for _ in 0..chars_to_delete {
                self.input.delete_next_char();
            }

            self.input.insert_str(file_path);

            self.file_suggestion.clear();
        }
    }

    pub async fn handle_event(&mut self, key_event: KeyEvent) -> UserAction {
        let now = Instant::now();
        self.last_keystroke_time = Some(now);

        // Convert any pending Enter to newline
        if self.pending_enter.is_some() {
            self.pending_enter = None;
            let fake_event = KeyEvent {
                code: KeyCode::Enter,
                modifiers: KeyModifiers::empty(),
                kind: key_event.kind,
                state: key_event.state,
            };
            let event: TextInput = Event::Key(fake_event).into();
            self.input.input(event);
        }

        let binding = key_event_to_binding(&key_event);

        // Check cancel_task / clear_input bindings
        if binding == self.cancel_task_binding || binding == self.clear_input_binding {
            if self.agent_running {
                return UserAction::CancelTask;
            }

            // Handle escape key for input clearing
            if let Some(escape_time) = self.escape_press_time {
                // Second escape within 1 second - clear input
                if escape_time.elapsed() < Duration::from_secs(1) {
                    self.input = TextArea::default();
                    self.escape_press_time = None;
                    self.helper_msg = None;
                    return UserAction::Nope;
                }
            }

            // First escape or escape after timeout - show message
            if !self.input.lines()[0].is_empty() {
                self.escape_press_time = Some(now);
                self.helper_set = Some(now);
                self.helper_duration = Some(Duration::from_secs(1));
                self.helper_msg = Some(" press esc again to clear".to_string());
            }
            return UserAction::Nope;
        }

        // Check paste binding
        if binding == self.paste_binding {
            // Handle Ctrl+V or Cmd+V paste directly from clipboard
            if let Ok(mut ctx) = Clipboard::new() {
                if let Ok(text) = ctx.get_text() {
                    self.input.insert_str(text);
                    return UserAction::Nope;
                }
            }
            // Fallback: let TextArea handle it normally
            let event: TextInput = Event::Key(key_event).into();
            self.input.input(event);
            return UserAction::Nope;
        }

        match key_event.code {
            KeyCode::Char('?') if self.input.lines()[0].is_empty() && self.help.is_none() => {
                self.help = Some(HelpArea);
            }
            KeyCode::Enter => {
                // Alt+Enter creates a new line immediately
                if key_event.modifiers.contains(KeyModifiers::ALT) {
                    self.last_keystroke_time = Some(now);

                    // Create fake Enter event without Alt modifier for TextArea
                    let fake_event = KeyEvent {
                        code: KeyCode::Enter,
                        modifiers: KeyModifiers::empty(),
                        kind: key_event.kind,
                        state: key_event.state,
                    };
                    let event: TextInput = Event::Key(fake_event).into();
                    self.input.input(event);
                    return UserAction::Nope;
                }

                // Tab to select current file suggestion
                if self.file_suggestion.is_active() {
                    if let Some(file_path) = self.file_suggestion.selected().map(|s| s.to_string())
                    {
                        self.replace_file_search(&file_path);
                    }
                    return UserAction::Nope;
                }

                // Tab to select current command suggestion
                if self.cmd_suggestion.is_active() {
                    if let Some(cmd) = self.cmd_suggestion.selected().map(|s| s.to_string()) {
                        self.input = TextArea::default();
                        self.input.insert_str(&cmd);
                        self.cmd_suggestion.clear();
                    }
                    return UserAction::Nope;
                }
                // Clear suggestions on Enter so message can be sent
                self.file_suggestion.clear();
                self.cmd_suggestion.clear();

                // Regular Enter - set pending and wait
                self.pending_enter = Some(now);
                return UserAction::Nope;
            }
            KeyCode::Up => {
                // If we have file suggestions, navigate through them
                if self.file_suggestion.is_active() {
                    self.file_suggestion.prev();
                    return UserAction::Nope;
                }
                // If we have command suggestions, navigate through them
                if self.cmd_suggestion.is_active() {
                    self.cmd_suggestion.prev();
                    return UserAction::Nope;
                }

                // Get current cursor position
                let cursor = self.input.cursor();
                let cursor_row = cursor.0;
                let is_empty = self.input.lines().iter().all(|line| line.is_empty());

                // Navigate history only if:
                // 1. Input is empty, OR
                // 2. Cursor is at the first line
                if !self.history.is_empty()
                    && self.history_index > 0
                    && (is_empty || cursor_row == 0)
                {
                    if self.history_index == self.history.len() && !is_empty {
                        let current_text = self.input.lines().join("\n");
                        self.current_draft = Some(current_text);
                    }

                    self.history_index -= 1;
                    self.load_historic_prompt(self.history_index);
                } else if !is_empty && cursor_row > 0 {
                    self.input.move_cursor(ratatui_textarea::CursorMove::Up);
                }
            }
            KeyCode::Down => {
                // If we have file suggestions, navigate through them
                if self.file_suggestion.is_active() {
                    self.file_suggestion.next();
                    return UserAction::Nope;
                }
                // If we have command suggestions, navigate through them
                if self.cmd_suggestion.is_active() {
                    self.cmd_suggestion.next();
                    return UserAction::Nope;
                }

                // Get current cursor position
                let cursor = self.input.cursor();
                let cursor_row = cursor.0;
                let is_empty = self.input.lines().iter().all(|line| line.is_empty());
                let line_count = self.input.lines().len();

                // Navigate history only if:
                // 1. Cursor is at the last line
                if !self.history.is_empty() && (is_empty || cursor_row == line_count - 1) {
                    if self.history_index < self.history.len() {
                        self.history_index += 1;
                        if self.history_index < self.history.len() {
                            self.load_historic_prompt(self.history_index);
                        } else {
                            // Restore draft or create empty input
                            if let Some(draft) = self.current_draft.take() {
                                self.input =
                                    TextArea::new(draft.lines().map(|s| s.to_string()).collect());
                                self.move_cursor_to_end_of_text();
                            } else {
                                self.input = TextArea::default();
                            }
                        }
                    }
                } else if !is_empty && cursor_row < line_count - 1 {
                    self.input.move_cursor(ratatui_textarea::CursorMove::Down);
                }
            }
            _ => {
                // Convert to ratatui event format for tui-textarea
                self.help = None;
                let event: Event = Event::Key(key_event);
                let input: TextInput = event.into();
                self.input.input(input);
            }
        }

        // Update suggestions after each keystroke
        self.file_suggestion.update(&self.input);
        let current_text = self.input.lines().join("\n");
        self.cmd_suggestion.update(&current_text);

        UserAction::Nope
    }
}

/// drawing logic
impl InputArea<'_> {
    pub fn height(&self) -> u16 {
        self.input.lines().len().max(1) as u16
            + 4
            + self.help.as_ref().map_or(0, |h| h.height())
            + self.file_suggestion.height()
            + self.cmd_suggestion.height()
    }

    pub fn draw(&mut self, f: &mut Frame, area: Rect) {
        let suggestions_height = self.file_suggestion.height();
        let cmd_suggestions_height = self.cmd_suggestion.height();

        let [status, input_area, cmd_suggestions_area, file_suggestions_area, helper, help_area] =
            Layout::vertical([
                Constraint::Length(1),
                Constraint::Length(self.input.lines().len().max(1) as u16 + 2),
                Constraint::Length(cmd_suggestions_height),
                Constraint::Length(suggestions_height),
                Constraint::Length(1),
                Constraint::Length(self.help.as_ref().map_or(0, |h| h.height())),
            ])
            .areas(area);

        // status
        f.render_widget(
            Span::styled(
                self.get_status_text(),
                Style::default().fg(self.palette.status),
            ),
            status,
        );

        // Input - clone and apply block styling
        let block = Block::default()
            .borders(Borders::ALL)
            .border_set(border::ROUNDED)
            .padding(Padding {
                left: 1,
                right: 1,
                top: 0,
                bottom: 0,
            })
            .border_style(Style::default().fg(self.palette.border));
        let inner = block.inner(input_area);
        f.render_widget(block, input_area);

        let [pad, prompt] =
            Layout::horizontal([Constraint::Length(2), Constraint::Fill(1)]).areas(inner);
        f.render_widget(">".to_string(), pad);

        // Set placeholder and block
        self.input.set_placeholder_text(&self.placeholder);
        self.input
            .set_placeholder_style(Style::default().fg(self.palette.placeholder));
        self.input
            .set_style(Style::default().fg(self.palette.input_text));
        self.input
            .set_cursor_style(Style::default().fg(self.palette.cursor_fg).bg(
                if !self.input.lines()[0].is_empty() {
                    self.palette.cursor_bg
                } else {
                    Color::Reset
                },
            ));
        self.input.set_cursor_line_style(Style::default());
        f.render_widget(&self.input, prompt);

        // Helper text area below input
        let [helper_left, _] =
            Layout::horizontal([Constraint::Fill(1), Constraint::Length(0)]).areas(helper);

        // Multi-line indicator
        let line_count = self.input.lines().len();
        let helper_text = self.check_helper_msg();
        let multi_line_indicator = if line_count > 1 {
            format!("{} [{} lines]", helper_text, line_count)
        } else {
            helper_text
        };
        f.render_widget(
            Span::styled(
                multi_line_indicator,
                Style::default().fg(self.palette.input_text),
            ),
            helper_left,
        );

        // Command suggestions
        self.cmd_suggestion
            .draw(f, cmd_suggestions_area, &self.palette);

        // File suggestions
        self.file_suggestion
            .draw(f, file_suggestions_area, &self.palette);

        // help
        if let Some(help) = &self.help {
            help.draw(f, help_area);
        }
    }
}
