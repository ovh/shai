use std::fs;

use jwalk::WalkDir;
use ratatui::{
    layout::Rect,
    style::{Style, Stylize},
    symbols::border,
    widgets::{Block, Borders, List, ListItem},
    Frame,
};
use ratatui_textarea::TextArea;

use super::theme::ThemePalette;

pub struct FileSuggestion {
    suggestions: Vec<String>,
    selected_index: Option<usize>,
    search: Option<String>,
    gitignore_patterns: Vec<String>,
}

impl FileSuggestion {
    pub fn new() -> Self {
        Self {
            suggestions: Vec::new(),
            selected_index: None,
            search: None,
            gitignore_patterns: Self::load_gitignore_patterns(),
        }
    }

    pub fn is_active(&self) -> bool {
        !self.suggestions.is_empty()
    }

    pub fn selected(&self) -> Option<&str> {
        self.selected_index
            .and_then(|idx| self.suggestions.get(idx).map(|s| s.as_str()))
    }

    pub fn clear(&mut self) {
        self.suggestions.clear();
        self.selected_index = None;
        self.search = None;
    }

    pub fn next(&mut self) {
        if let Some(idx) = self.selected_index {
            self.selected_index = Some((idx + 1) % self.suggestions.len());
        }
    }

    pub fn prev(&mut self) {
        if let Some(idx) = self.selected_index {
            self.selected_index = Some(if idx > 0 {
                idx - 1
            } else {
                self.suggestions.len() - 1
            });
        }
    }

    pub fn height(&self) -> u16 {
        if self.suggestions.is_empty() {
            0
        } else {
            self.suggestions.len().min(5) as u16 + 2
        }
    }

    pub fn update(&mut self, input: &TextArea<'_>) {
        if let Some((_at_pos, search)) = Self::detect_file_search(input) {
            if self.search.as_deref() != Some(&search) {
                self.search = Some(search.clone());
                self.suggestions = self.search_files(&search);
                self.selected_index = if self.suggestions.is_empty() {
                    None
                } else {
                    Some(0)
                };
            }
        } else {
            self.clear();
        }
    }

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

    fn should_ignore(path: &str, patterns: &[String]) -> bool {
        let path_clean = path.trim_start_matches("./");
        for pattern in patterns {
            let pattern_clean = pattern.trim_start_matches("./").trim_end_matches('/');

            if path_clean == pattern_clean || path_clean.starts_with(&format!("{}/", pattern_clean))
            {
                return true;
            }

            if pattern.contains('*') {
                let parts: Vec<&str> = pattern.split('*').collect();
                if parts.len() == 2
                    && path_clean.starts_with(parts[0])
                    && path_clean.ends_with(parts[1])
                {
                    return true;
                }
            }
        }
        false
    }

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

    pub fn detect_file_search(input: &TextArea<'_>) -> Option<(usize, String)> {
        let cursor = input.cursor();
        let (row, col) = (cursor.0, cursor.1);
        let line = input.lines().get(row)?;

        let chars: Vec<char> = line.chars().collect();
        let col_safe = col.min(chars.len());

        let before_cursor: String = chars.iter().take(col_safe).collect();
        if let Some(at_pos) = before_cursor.rfind('@') {
            let after_at: String = before_cursor.chars().skip(at_pos + 1).collect();
            if !after_at.contains(' ') {
                let at_char_pos = before_cursor.chars().take(at_pos).count();
                return Some((at_char_pos, after_at));
            }
        }
        None
    }

    pub fn draw(&self, f: &mut Frame, area: Rect, palette: &ThemePalette) {
        if self.suggestions.is_empty() {
            return;
        }

        let max_visible = 5;
        let total = self.suggestions.len();
        let selected = self.selected_index.unwrap_or(0);

        let start = if total <= max_visible {
            0
        } else {
            let ideal_start = selected.saturating_sub(max_visible / 2);
            ideal_start.min(total.saturating_sub(max_visible))
        };

        let end = (start + max_visible).min(total);

        let items: Vec<ListItem> = self.suggestions[start..end]
            .iter()
            .enumerate()
            .map(|(window_idx, path)| {
                let actual_idx = start + window_idx;
                let style = if Some(actual_idx) == self.selected_index {
                    Style::default()
                        .fg(palette.suggestion_selected_fg)
                        .bg(palette.suggestion_selected_bg)
                } else {
                    Style::default().fg(palette.suggestion_normal)
                };
                ListItem::new(path.as_str()).style(style)
            })
            .collect();

        let title = if total > max_visible {
            format!("Files ({}/{})", selected + 1, total)
        } else {
            "Files".to_string()
        };

        let suggestions_list = List::new(items).block(
            Block::default()
                .borders(Borders::ALL)
                .border_set(border::ROUNDED)
                .border_style(Style::default().fg(palette.border))
                .title(title),
        );

        f.render_widget(suggestions_list, area);
    }
}

use super::command::CommandRegistry;

pub struct CommandSuggestion {
    suggestions: Vec<String>,
    selected_index: Option<usize>,
}

impl CommandSuggestion {
    fn all_commands() -> Vec<&'static str> {
        CommandRegistry::commands().iter().map(|c| c.name).collect()
    }

    pub fn new() -> Self {
        Self {
            suggestions: Vec::new(),
            selected_index: None,
        }
    }

    pub fn is_active(&self) -> bool {
        !self.suggestions.is_empty()
    }

    pub fn selected(&self) -> Option<&str> {
        self.selected_index
            .and_then(|idx| self.suggestions.get(idx).map(|s| s.as_str()))
    }

    pub fn clear(&mut self) {
        self.suggestions.clear();
        self.selected_index = None;
    }

    pub fn next(&mut self) {
        if let Some(idx) = self.selected_index {
            self.selected_index = Some((idx + 1) % self.suggestions.len());
        }
    }

    pub fn prev(&mut self) {
        if let Some(idx) = self.selected_index {
            self.selected_index = Some(if idx > 0 {
                idx - 1
            } else {
                self.suggestions.len() - 1
            });
        }
    }

    pub fn height(&self) -> u16 {
        if self.suggestions.is_empty() {
            0
        } else {
            self.suggestions.len().min(5) as u16 + 2
        }
    }

    pub fn update(&mut self, input: &str) {
        if input.starts_with('/') && !input.contains(' ') {
            let prefix = input.trim();
            let all = Self::all_commands();
            let filtered: Vec<String> = all
                .iter()
                .filter(|cmd| cmd.starts_with(prefix))
                .map(|s| s.to_string())
                .collect();
            if filtered.is_empty() || (filtered.len() == all.len() && prefix == "/")
            {
                self.suggestions = all.iter().map(|s| s.to_string()).collect();
            } else {
                self.suggestions = filtered;
            }
            self.selected_index = if self.suggestions.is_empty() {
                None
            } else {
                Some(0)
            };
        } else {
            self.clear();
        }
    }

    pub fn draw(&self, f: &mut Frame, area: Rect, palette: &ThemePalette) {
        if self.suggestions.is_empty() {
            return;
        }

        let max_visible = 5;
        let total = self.suggestions.len();
        let selected = self.selected_index.unwrap_or(0);

        let start = if total <= max_visible {
            0
        } else {
            let ideal_start = selected.saturating_sub(max_visible / 2);
            ideal_start.min(total.saturating_sub(max_visible))
        };

        let end = (start + max_visible).min(total);

        let items: Vec<ListItem> = self.suggestions[start..end]
            .iter()
            .enumerate()
            .map(|(window_idx, cmd)| {
                let actual_idx = start + window_idx;
                let style = if Some(actual_idx) == self.selected_index {
                    Style::default()
                        .fg(palette.suggestion_selected_fg)
                        .bg(palette.suggestion_selected_bg)
                } else {
                    Style::default().fg(palette.suggestion_normal)
                };
                ListItem::new(cmd.as_str()).style(style)
            })
            .collect();

        let suggestions_list = List::new(items).block(
            Block::default()
                .borders(Borders::ALL)
                .border_set(border::ROUNDED)
                .border_style(Style::default().fg(palette.border))
                .title("Commands"),
        );

        f.render_widget(suggestions_list, area);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_file_suggestion_new() {
        let fs = FileSuggestion::new();
        assert!(!fs.is_active());
        assert_eq!(fs.height(), 0);
    }

    #[test]
    fn test_command_suggestion_new() {
        let cs = CommandSuggestion::new();
        assert!(!cs.is_active());
        assert_eq!(cs.height(), 0);
    }

    #[test]
    fn test_command_suggestion_update_matching_prefix() {
        let mut cs = CommandSuggestion::new();
        cs.update("/exi");
        assert!(cs.is_active());
        assert_eq!(cs.suggestions, vec!["/exit"]);
    }

    #[test]
    fn test_command_suggestion_update_no_match() {
        let mut cs = CommandSuggestion::new();
        // When no commands match, all commands are shown (matches original behavior)
        cs.update("/xyz");
        assert!(cs.is_active());
    }

    #[test]
    fn test_command_suggestion_update_clears_on_space() {
        let mut cs = CommandSuggestion::new();
        cs.update("/exit");
        assert!(cs.is_active());
        cs.update("/exit hello");
        assert!(!cs.is_active());
    }

    #[test]
    fn test_command_suggestion_next_prev() {
        let mut cs = CommandSuggestion::new();
        cs.update("/");
        assert!(cs.is_active());
        let len = cs.suggestions.len();
        assert_eq!(cs.selected_index, Some(0));

        cs.next();
        assert_eq!(cs.selected_index, Some(1));

        cs.prev();
        assert_eq!(cs.selected_index, Some(0));

        cs.prev();
        assert_eq!(cs.selected_index, Some(len - 1));

        cs.next();
        assert_eq!(cs.selected_index, Some(0));
    }
}
