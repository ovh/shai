use std::time::{Instant, Duration};
use std::fs;
use std::path::Path;

use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use futures::io;
use cli_clipboard::{ClipboardContext, ClipboardProvider};
use jwalk::WalkDir;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style, Stylize},
    symbols::border,
    text::{Line, Span},
    widgets::{Block, Borders, Padding, Paragraph, Widget, List, ListItem},
    Frame,
};
use shai_core::agent::{AgentController, AgentEvent, PublicAgentState};
use shai_llm::{tool::call_fc_auto::ToolCallFunctionCallingAuto, ToolCallMethod};
use tui_textarea::{Input, TextArea};

use crate::{tui::{cmdnav::CommandNav, helper::HelpArea}};

use super::theme::{SHAI_YELLOW, ThemePalette};

pub enum UserAction {
    Nope,
    CancelTask,
    UserInput {
        input: String
    },
    UserAppCommand {
        command: String
    }
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

    // bottom helper
    help: Option<HelpArea>,
    cmdnav: CommandNav,

    history: Vec<String>,
    history_index: usize,

    // file suggestions
    file_suggestions: Vec<String>,
    suggestion_index: Option<usize>,
    suggestion_search: Option<String>,

    // gitignore patterns (loaded once)
    gitignore_patterns: Vec<String>,
    
    // theme colors
    palette: ThemePalette,
    
    // Track if we just entered a backslash to allow multiline
    just_entered_backslash: bool,
}

impl InputArea<'_> {
    pub fn new(palette: ThemePalette) -> Self {
        Self {
            agent_running: false,
            input: TextArea::default(),
            placeholder: "? for shortcuts".to_string(),
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
            help: None,
            cmdnav: CommandNav{},
            history: Vec::new(),
            history_index: 0,
            file_suggestions: Vec::new(),
            suggestion_index: None,
            suggestion_search: None,
            gitignore_patterns: Self::load_gitignore_patterns(),
            palette,
            just_entered_backslash: false,
        }
    }

    pub fn set_history(&mut self, history: Vec<String>) {
        self.history = history;
        self.history_index = self.history.len();
    }

    pub fn set_palette(&mut self, palette: ThemePalette) {
        self.palette = palette;
    }

    // Parse .gitignore and return list of patterns to ignore
    fn load_gitignore_patterns() -> Vec<String> {
        if let Ok(content) = fs::read_to_string(".gitignore") {
            content
                .lines()
                .filter_map(|line| {
                    let trimmed = line.trim();
                    if trimmed.is_empty() || trimmed.starts_with('#') {
                        None
                    } else {
                        Some(trimmed.to_string())
                    }
                })
                .collect()
        } else {
            Vec::new()
        }
    }

    // Check if a path should be ignored based on gitignore patterns
    fn should_ignore(path: &str, patterns: &[String]) -> bool {
        for pattern in patterns {
            let pattern_clean = pattern.trim_start_matches("./");
            
            if path.contains(pattern_clean) {
                return true;
            }
            
            if pattern.ends_with('/') {
                let dir_pattern = pattern.trim_end_matches('/');
                if path.contains(dir_pattern) {
                    return true;
                }
            }
            
            if pattern.contains('*') {
                let parts: Vec<&str> = pattern.split('*').collect();
                if parts.len() == 2 {
                    if path.contains(parts[0]) && path.ends_with(parts[1]) {
                        return true;
                    }
                }
            }
        }
        false
    }

    // Detect if cursor is after a @ and extract the search text
    fn detect_file_search(&self) -> Option<(usize, String)> {
        let (row, col) = self.input.cursor();
        let line = self.input.lines().get(row)?;

        // Use character indices, not byte indices
        let chars: Vec<char> = line.chars().collect();
        let col_safe = col.min(chars.len());

        // Look for the last @ before the cursor
        let before_cursor: String = chars.iter().take(col_safe).collect();
        if let Some(at_pos) = before_cursor.rfind('@') {
            // Check there's no space between @ and cursor
            let after_at: String = before_cursor.chars().skip(at_pos + 1).collect();
            if !after_at.contains(' ') {
                // Return position in character count (not bytes)
                let at_char_pos = before_cursor.chars().take(at_pos).count();
                return Some((at_char_pos, after_at));
            }
        }
        None
    }

    // Search files matching the pattern - optimized with jwalk and respecting .gitignore
    fn search_files(&self, pattern: &str) -> Vec<String> {
        let pattern_lower = pattern.to_lowercase();
        let include_hidden = pattern.starts_with('.');
        
        WalkDir::new(".")
            .max_depth(5)
            .skip_hidden(!include_hidden)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter_map(|e| {
                let path = e.path();
                let path_str = path.to_string_lossy().to_string();
                
                // Skip if matches gitignore patterns
                if Self::should_ignore(&path_str, &self.gitignore_patterns) {
                    return None;
                }
                
                if pattern.is_empty() || path_str.to_lowercase().contains(&pattern_lower) {
                    Some(path_str)
                } else {
                    None
                }
            })
            .take(20)
            .collect()
    }

    // Update suggestions based on current input
    fn update_suggestions(&mut self) {
        if let Some((at_pos, search)) = self.detect_file_search() {
            if self.suggestion_search.as_ref() != Some(&search) {
                self.suggestion_search = Some(search.clone());
                self.file_suggestions = self.search_files(&search);
                self.suggestion_index = if self.file_suggestions.is_empty() {
                    None
                } else {
                    Some(0)
                };
            }
        } else {
            self.file_suggestions.clear();
            self.suggestion_index = None;
            self.suggestion_search = None;
        }
    }
}


/// method info bottom right
impl InputArea<'_> {
    pub fn set_tool_call_method(&mut self, method: ToolCallMethod) {
        self.method = method;
    }

    pub fn method_str(&self) -> &str {
        match self.method {
            ToolCallMethod::Auto => {
                "🛠️ tool call try all methods"
            }
            ToolCallMethod::FunctionCall => {
                "🛠️ function call (auto)"
            }
            ToolCallMethod::FunctionCallRequired => {
                "🛠️ function call (required)"
            }
            ToolCallMethod::StructuredOutput => {
                "🛠️ structured output"
            }
            ToolCallMethod::Parsing => {
                "🛠️ parsing"
            }
        }
    } 
}


/// alert message in yellow, top left
impl InputArea<'_> {
    pub fn set_agent_running(&mut self, running: bool) {
        self.agent_running = running;
        if running {
            self.animation_start = Some(Instant::now());
        } else {
            self.status_message = None;
            self.animation_start = None;
        }
    }

    pub fn with_placeholder(mut self, placeholder: &str) -> Self {
        self.placeholder = placeholder.to_string();
        self
    }

    pub fn set_status(&mut self, text: &str) {
        self.status_message = Some(text.to_string());
    }

    pub fn is_animating(&self) -> bool {
        self.animation_start.is_some()
    }

    fn get_status_text(&self) -> String {
        if let Some(ref msg) = self.status_message {
            // Show status message if we have one (like "Task cancelled")
            format!(" {}", msg)
        } else if let Some(animation_start) = self.animation_start {
            // Show spinner when agent is working
            let spinner_chars = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
            let elapsed = animation_start.elapsed().as_millis();
            let index = (elapsed / 100) % spinner_chars.len() as u128;
            format!(" {} Agent is working... (press esc to cancel)", spinner_chars[index as usize])
        } else {
            // Agent is waiting for input, no status to show
            String::new()
        }
    }
}

/// status message bottom left
impl InputArea<'_> {
    pub fn alert_msg(&mut self, text: &str, duration: Duration) {
        self.helper_msg = Some(text.to_string());
        self.helper_set = Some(Instant::now());
        self.helper_duration = Some(duration);
    }

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
                    
                    // Handle app commands vs agent input
                    self.input = TextArea::default();
                    if input.starts_with('/') {
                        return Some(UserAction::UserAppCommand { 
                            command: input
                         });
                    } else {
                        // Remove backslashes from the final input
                        let cleaned_input = self.remove_backslashes_from_input(&input);
                        return Some(UserAction::UserInput { 
                            input: cleaned_input
                        });
                    }
                }
            }
        }
        None
    }

    /// Remove backslashes that were used for multiline input
    fn remove_backslashes_from_input(&self, input: &str) -> String {
        let lines: Vec<&str> = input.lines().collect();
        let mut cleaned_lines: Vec<String> = Vec::new();
        
        for line in lines {
            if line.ends_with('\\') && line.chars().count() > 1 {
                // Remove the backslash from the line - this preserves the newline
                let cleaned_line: String = line[..line.len() - 1].to_string();
                cleaned_lines.push(cleaned_line);
            } else {
                cleaned_lines.push(line.to_string());
            }
        }
        
        cleaned_lines.join("\n")
    }

    fn check_helper_msg(&mut self) -> String {
        // Check if escape message should be cleared after 1 second
        if let Some(helper_time) = self.helper_set {
            if helper_time.elapsed() >= self.helper_duration.unwrap() {
                self.helper_msg = None;
                self.helper_set = None;
                self.helper_duration = None;
                return String::new();
            }
        }
        
        // Return current helper message or empty string
        self.helper_msg.as_deref().unwrap_or("").to_string()
    }
}


/// event related
impl InputArea<'_> {
    fn move_cursor_to_end_of_text(&mut self) {
        for _ in 0..self.input.lines().len().saturating_sub(1) {
            self.input.move_cursor(tui_textarea::CursorMove::Down);
        }
        if let Some(last_line) = self.input.lines().last() {
            for _ in 0..last_line.len() {
                self.input.move_cursor(tui_textarea::CursorMove::Forward);
            }
        }
    }

    fn load_historic_prompt(&mut self, index: usize) {
        if let Some(entry) = self.history.get(index) {
            self.input = TextArea::new(entry.lines().map(|s| s.to_string()).collect());
            self.move_cursor_to_end_of_text();
        }
    }

    pub async fn handle_event(&mut self, key_event: KeyEvent) -> UserAction{
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
            let event: Input = Event::Key(fake_event).into();
            self.input.input(event);
        }
        
        match key_event.code {
            KeyCode::Char('?') if self.input.lines()[0].is_empty() && self.help.is_none() => {
                self.help = Some(HelpArea);
            }
            KeyCode::Esc => {
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
            }
            KeyCode::Char('v') if key_event.modifiers.contains(KeyModifiers::CONTROL) || key_event.modifiers.contains(KeyModifiers::SUPER) => {                
                // Handle Ctrl+V or Cmd+V paste directly from clipboard
                if let Ok(mut ctx) = ClipboardContext::new() {
                    if let Ok(text) = ctx.get_contents() {
                        self.input.insert_str(text);
                        return UserAction::Nope;
                    }
                }
                // Fallback: let TextArea handle it normally
                let event: Input = Event::Key(key_event).into();
                self.input.input(event);
                return UserAction::Nope;
            }
            KeyCode::Char('\\') => {
                // Handle backslash input - mark that we just entered a backslash
                self.just_entered_backslash = true;
                let event: Event = Event::Key(KeyEvent::from(key_event));
                let input: Input = event.into();
                self.input.input(input);
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
                    let event: Input = Event::Key(fake_event).into();
                    self.input.input(event);
                    return UserAction::Nope;
                }

                // If we just entered a backslash, treat this as a newline instead of submission
                if self.just_entered_backslash {
                    self.just_entered_backslash = false;
                    self.last_keystroke_time = Some(now);
                    
                    // Create fake Enter event without Alt modifier for TextArea
                    let fake_event = KeyEvent {
                        code: KeyCode::Enter,
                        modifiers: KeyModifiers::empty(),
                        kind: key_event.kind,
                        state: key_event.state,
                    };
                    let event: Input = Event::Key(fake_event).into();
                    self.input.input(event);
                    return UserAction::Nope;
                }

                // Tab to select current suggestion
                if let Some(idx) = self.suggestion_index {
                    if let Some(file_path) = self.file_suggestions.get(idx).cloned() {
                        self.replace_file_search(&file_path);
                    }
                    return UserAction::Nope;
                }
                // Clear suggestions on Enter so message can be sent
                self.file_suggestions.clear();
                self.suggestion_index = None;
                self.suggestion_search = None;

                // Regular Enter - set pending and wait
                self.pending_enter = Some(now);
                return UserAction::Nope;
            }
            KeyCode::Up => {
                // If we have suggestions, navigate through them
                if !self.file_suggestions.is_empty() {
                    if let Some(idx) = self.suggestion_index {
                        self.suggestion_index = Some(if idx > 0 { idx - 1 } else { self.file_suggestions.len() - 1 });
                    }
                    return UserAction::Nope;
                }

                // Get current cursor position
                let (cursor_row, _) = self.input.cursor();
                let is_empty = self.input.lines().iter().all(|line| line.is_empty());

                // Navigate history only if:
                // 1. Input is empty, OR
                // 2. Cursor is at the first line
                if !self.history.is_empty() && self.history_index > 0 && (is_empty || cursor_row == 0) {
                    if self.history_index == self.history.len() && !is_empty {
                        let current_text = self.input.lines().join("\n");
                        self.current_draft = Some(current_text);
                    }

                    self.history_index -= 1;
                    self.load_historic_prompt(self.history_index);
                } else if !is_empty && cursor_row > 0 {
                    self.input.move_cursor(tui_textarea::CursorMove::Up);
                }
            }
            KeyCode::Down => {
                // If we have suggestions, navigate through them
                if !self.file_suggestions.is_empty() {
                    if let Some(idx) = self.suggestion_index {
                        self.suggestion_index = Some((idx + 1) % self.file_suggestions.len());
                    }
                    return UserAction::Nope;
                }

                // Get current cursor position
                let (cursor_row, _) = self.input.cursor();
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
                                self.input = TextArea::new(draft.lines().map(|s| s.to_string()).collect());
                                self.move_cursor_to_end_of_text();
                            } else {
                                self.input = TextArea::default();
                            }
                        }
                    }
                } else if !is_empty && cursor_row < line_count - 1 {
                    self.input.move_cursor(tui_textarea::CursorMove::Down);
                }
            }
            _ => {
                // Convert to ratatui event format for tui-textarea
                self.help = None;
                let event: Event = Event::Key(KeyEvent::from(key_event));
                let input: Input = event.into();
                self.input.input(input);
                
                // Reset the backslash flag when any key other than Enter is pressed
                if key_event.code != KeyCode::Enter {
                    self.just_entered_backslash = false;
                }
            }
        }

        // Update suggestions after each keystroke
        self.update_suggestions();

        UserAction::Nope
    }

    // Replace @search with the file path
    fn replace_file_search(&mut self, file_path: &str) {
        if let Some((at_pos, search_text)) = self.detect_file_search() {
            let (row, _) = self.input.cursor();

            // Calculate how many characters to delete (@ + search text)
            let chars_to_delete = 1 + search_text.len(); // @ + text after

            // Move cursor to @ position
            self.input.move_cursor(tui_textarea::CursorMove::Head);
            for _ in 0..at_pos {
                self.input.move_cursor(tui_textarea::CursorMove::Forward);
            }

            // Delete @ + search text
            for _ in 0..chars_to_delete {
                self.input.delete_next_char();
            }

            // Insert file path
            self.input.insert_str(file_path);

            // Reset suggestions
            self.file_suggestions.clear();
            self.suggestion_index = None;
            self.suggestion_search = None;
        }
    }
}


/// drawing logic
impl InputArea<'_> {
    pub fn height(&self) -> u16 {
        // +2 for top/bottom borders
        // +N for lines inside input
        // +1 for helper text below input
        let suggestions_height = if !self.file_suggestions.is_empty() {
            self.file_suggestions.len().min(5) as u16 + 2
        } else {
            0
        };
        self.input.lines().len().max(1) as u16 + 4 + self.help.as_ref().map_or(0, |h| h.height()) + suggestions_height
    }

    pub fn draw(&mut self, f: &mut Frame, area: Rect) {
        let suggestions_height = if !self.file_suggestions.is_empty() {
            self.file_suggestions.len().min(5) as u16 + 2
        } else {
            0
        };

        let [status, input_area, suggestions_area, helper, help_area] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(self.height() - 2 - suggestions_height),
            Constraint::Length(suggestions_height),
            Constraint::Length(1),
            Constraint::Length(self.help.as_ref().map_or(0, |h| h.height()))
        ]).areas(area);
        
        // status
        f.render_widget(Span::styled(self.get_status_text(), Style::default().fg(self.palette.status)), status);

        // Input - clone and apply block styling
        let block = Block::default()
            .borders(Borders::ALL)
            .border_set(border::ROUNDED)
            .padding(Padding { left: 1, right: 1, top: 0, bottom: 0 })
            .border_style(Style::default().fg(self.palette.border));
        let inner = block.inner(input_area);
        f.render_widget(block, input_area);

        let [pad, prompt] = Layout::horizontal([Constraint::Length(2), Constraint::Fill(1)]).areas(inner);
        f.render_widget(format!(">"), pad);

        // Set placeholder and block
        self.input.set_placeholder_text("? for help");
        self.input.set_placeholder_style(Style::default().fg(self.palette.placeholder));
        self.input.set_style(Style::default().fg(self.palette.input_text));
        self.input.set_cursor_style(Style::default()
            .fg(self.palette.cursor_fg)
            .bg(if !self.input.lines()[0].is_empty() { self.palette.cursor_bg } else { Color::Reset }));
        self.input.set_cursor_line_style(Style::default());
        f.render_widget(&self.input, prompt);
        
        // Helper text area below input
        let [helper_left, _, helper_right] = Layout::horizontal([
            Constraint::Fill(1), 
            Constraint::Fill(1), 
            Constraint::Length(self.method_str().len() as u16)
        ]).areas(helper);

        let helper_text = self.check_helper_msg();
        f.render_widget(
            Span::styled(helper_text, Style::default().fg(self.palette.method_label).dim()), 
            helper_left
        );
                
        // Status
        f.render_widget(
            Span::styled(self.method_str(), Style::default().fg(self.palette.method_label)), 
            helper_right
        );

        // File suggestions
        if !self.file_suggestions.is_empty() {
            let max_visible = 5;
            let total = self.file_suggestions.len();
            let selected = self.suggestion_index.unwrap_or(0);
            
            // Calculate scrolling window
            let start = if total <= max_visible {
                0
            } else {
                // Center the selected item in the window when possible
                let ideal_start = selected.saturating_sub(max_visible / 2);
                ideal_start.min(total.saturating_sub(max_visible))
            };
            
            let end = (start + max_visible).min(total);
            
            let items: Vec<ListItem> = self.file_suggestions[start..end]
                .iter()
                .enumerate()
                .map(|(window_idx, path)| {
                    let actual_idx = start + window_idx;
                    let style = if Some(actual_idx) == self.suggestion_index {
                        Style::default().fg(self.palette.suggestion_selected_fg).bg(self.palette.suggestion_selected_bg)
                    } else {
                        Style::default().fg(self.palette.suggestion_normal)
                    };
                    ListItem::new(path.as_str()).style(style)
                })
                .collect();

            let title = if total > max_visible {
                format!("Files ({}/{})", selected + 1, total)
            } else {
                "Files".to_string()
            };

            let suggestions_list = List::new(items)
                .block(Block::default()
                    .borders(Borders::ALL)
                    .border_set(border::ROUNDED)
                    .border_style(Style::default().fg(self.palette.border))
                    .title(title));

            f.render_widget(suggestions_list, suggestions_area);
        }

        // help
        if let Some(help) = &self.help {
            help.draw(f, help_area);
        }
    }
}