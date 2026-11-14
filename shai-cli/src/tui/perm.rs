use std::cell::Cell;
use std::time::Duration;

use ansi_to_tui::IntoText;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style, Stylize},
    symbols::border,
    text::{Line, Span, Text},
    widgets::{
        Block, Borders, List, ListDirection, ListItem, Padding, Paragraph, Scrollbar,
        ScrollbarOrientation, ScrollbarState, Widget,
    },
    Frame,
};
use shai_core::{
    agent::{events::PermissionRequest, output::PrettyFormatter, PermissionResponse},
    tools::{ToolCall, ToolResult},
};
// Removed ratatui_textarea dependency for colored preview

use super::theme::ThemePalette;

/// Columns moved per Left/Right key press in the preview.
const H_SCROLL_STEP: usize = 4;

pub enum PermissionModalAction {
    Nope,
    Response {
        request_id: String,
        choice: PermissionResponse,
    },
}

#[derive(Clone)]
pub struct PermissionWidget<'a> {
    pub request_id: String,
    pub request: PermissionRequest,
    pub remaining_perms: usize,

    selected_index: usize,
    preview_text: Text<'a>,
    scroll_offset: usize,
    h_scroll_offset: usize,
    /// Width of the preview pane at the last draw (0 before the first draw).
    preview_width: Cell<u16>,
    scroll_state: ScrollbarState,
    palette: ThemePalette,
}

impl PermissionWidget<'_> {
    pub fn new(
        request_id: String,
        request: PermissionRequest,
        total: usize,
        palette: ThemePalette,
    ) -> Self {
        let formatter = PrettyFormatter::new();
        let formatted_request = formatter.format_toolcall(&request.call, request.preview.as_ref());
        let mut preview_text = formatted_request
            .into_text()
            .unwrap_or_else(|_| Text::from(formatted_request.clone()));
        super::history::patch_default_style(
            &mut preview_text,
            palette.input_text,
            palette.background,
        );
        let content_length = preview_text.lines.len();

        Self {
            request_id,
            request,
            selected_index: 0,
            remaining_perms: total,
            preview_text,
            scroll_offset: 0,
            h_scroll_offset: 0,
            preview_width: Cell::new(0),
            scroll_state: ScrollbarState::new(content_length),
            palette,
        }
    }

    pub fn move_up(&mut self) {
        self.selected_index = if self.selected_index == 0 {
            2
        } else {
            self.selected_index - 1
        };
    }

    pub fn move_down(&mut self) {
        self.selected_index = (self.selected_index + 1) % 3;
    }

    pub fn scroll_up(&mut self) {
        self.scroll_offset = self.scroll_offset.saturating_sub(1);
        self.scroll_state = self.scroll_state.position(self.scroll_offset);
    }

    pub fn scroll_down(&mut self) {
        let max_scroll = self.preview_text.lines.len().saturating_sub(1);
        if self.scroll_offset < max_scroll {
            self.scroll_offset += 1;
            self.scroll_state = self.scroll_state.position(self.scroll_offset);
        }
    }

    pub fn scroll_left(&mut self) {
        // Clamp first so Left reacts at once if the pane grew since the last press.
        self.h_scroll_offset = self
            .h_scroll_offset
            .min(self.max_h_scroll())
            .saturating_sub(H_SCROLL_STEP);
    }

    pub fn scroll_right(&mut self) {
        self.h_scroll_offset = (self.h_scroll_offset + H_SCROLL_STEP).min(self.max_h_scroll());
    }

    fn max_line_width(&self) -> usize {
        self.preview_text
            .lines
            .iter()
            .map(|line| line.width())
            .max()
            .unwrap_or(0)
    }

    /// Largest horizontal offset that still fills the preview pane.
    fn max_h_scroll(&self) -> usize {
        match self.preview_width.get() as usize {
            0 => self.max_line_width().saturating_sub(1),
            width => self.max_line_width().saturating_sub(width),
        }
    }

    pub fn get_selected(&self) -> PermissionResponse {
        match self.selected_index {
            0 => PermissionResponse::Allow,
            1 => PermissionResponse::AllowAlways,
            2 => PermissionResponse::Deny,
            _ => PermissionResponse::Deny,
        }
    }

    pub fn handle_key_event(&mut self, key_event: KeyEvent) -> PermissionModalAction {
        match key_event.code {
            KeyCode::Up => {
                self.move_up();
                PermissionModalAction::Nope
            }
            KeyCode::Down => {
                self.move_down();
                PermissionModalAction::Nope
            }
            KeyCode::PageUp => {
                // Scroll preview up
                for _ in 0..5 {
                    self.scroll_up();
                }
                PermissionModalAction::Nope
            }
            KeyCode::PageDown => {
                // Scroll preview down
                for _ in 0..5 {
                    self.scroll_down();
                }
                PermissionModalAction::Nope
            }
            KeyCode::Left => {
                // Scroll preview left
                self.scroll_left();
                PermissionModalAction::Nope
            }
            KeyCode::Right => {
                // Scroll preview right
                self.scroll_right();
                PermissionModalAction::Nope
            }
            KeyCode::Enter => {
                let request_id = self.request_id.clone();
                let choice = self.get_selected();
                PermissionModalAction::Response { request_id, choice }
            }
            KeyCode::Esc => {
                let request_id = self.request_id.clone();
                let choice = PermissionResponse::Deny;
                PermissionModalAction::Response { request_id, choice }
            }
            _ => PermissionModalAction::Nope,
        }
    }

    pub fn height(&self) -> u16 {
        4 // outer permission block 2 + 1 top padding
       + 2 // inner tool preview block 2 (0 padding)
       + self.preview_text.lines.len() as u16  // preview content
       + 4 // allow, yolo, deny + 1 top space
    }

    pub fn draw(&self, f: &mut Frame, area: Rect) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_set(border::ROUNDED)
            .padding(Padding {
                left: 1,
                right: 1,
                top: 1,
                bottom: 1,
            })
            .border_style(Style::default().fg(self.palette.status))
            .title(if self.remaining_perms > 1 {
                format!(" 🔐 Permission Required ({}/{}) ", 1, self.remaining_perms)
            } else {
                " 🔐 Permission Required ".to_string()
            });

        let inner = block.inner(area);
        f.render_widget(block, area);

        let [tool, modal] = Layout::vertical([
            Constraint::Length(self.preview_text.lines.len() as u16 + 2),
            Constraint::Length(4),
        ])
        .areas(inner);

        let call = self.request.call.clone();
        let tool_name = PrettyFormatter::capitalize_first(&call.tool_name);
        let context = PrettyFormatter::extract_primary_param(&call.parameters, &call.tool_name);
        let mut title = Line::from(vec![
            Span::styled("🔧 ", self.palette.input_text),
            Span::styled(
                tool_name,
                Style::default().fg(self.palette.input_text).bold(),
            ),
        ]);
        if let Some((_, ctx)) = context {
            title.push_span(Span::styled(
                format!("({})", ctx),
                Style::default().fg(self.palette.input_text),
            ));
        };

        let mut block = Block::default()
            .borders(Borders::ALL)
            .border_set(border::ROUNDED)
            .padding(Padding {
                left: 1,
                right: 1,
                top: 0,
                bottom: 0,
            })
            .title(title.fg(self.palette.input_text))
            .border_style(Style::default().fg(self.palette.border));

        let inner = block.inner(tool);
        // The vertical scrollbar takes the rightmost column, so text gets one column less.
        // It is only drawn with room for a track between its two arrows (height > 2).
        let show_scrollbar =
            self.preview_text.lines.len() > inner.height as usize && inner.height > 2;
        let text_area = if show_scrollbar {
            Rect {
                width: inner.width.saturating_sub(1),
                ..inner
            }
        } else {
            inner
        };
        self.preview_width.set(text_area.width);
        if self.max_line_width() > text_area.width as usize {
            block = block.title_bottom(
                Line::from(" ←/→ scroll ")
                    .right_aligned()
                    .fg(self.palette.placeholder),
            );
        }
        f.render_widget(block, tool);

        // Render scrollable paragraph with colors
        let h_offset = self.h_scroll_offset.min(self.max_h_scroll());
        let paragraph = Paragraph::new(self.preview_text.clone())
            .scroll((self.scroll_offset as u16, h_offset as u16));
        f.render_widget(paragraph, text_area);

        // Render scrollbar if content is longer than area
        if show_scrollbar {
            let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .style(Style::default().fg(self.palette.border));
            f.render_stateful_widget(scrollbar, inner, &mut self.scroll_state.clone());
        }

        let items = ["Allow", "Use auto mode", "Deny"];
        let mut lines = vec![Line::from("Do you want to run this tool?")];
        for (i, s) in items.into_iter().enumerate() {
            if i == self.selected_index {
                lines.push(Line::from(vec![
                    Span::styled("❯ ", self.palette.suggestion_selected_fg),
                    Span::styled(s, self.palette.suggestion_selected_fg),
                ]));
            } else {
                lines.push(Line::from(vec![
                    Span::styled("  ", self.palette.placeholder),
                    Span::styled(s, self.palette.placeholder),
                ]));
            };
        }
        let text = Text::from(lines);
        let p = Paragraph::new(text);
        f.render_widget(p, modal);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyModifiers;
    use ratatui::{backend::TestBackend, Terminal};

    fn make_widget(preview: &str) -> PermissionWidget<'static> {
        let request = PermissionRequest {
            tool_name: "bash".to_string(),
            operation: "run command".to_string(),
            call: ToolCall {
                tool_call_id: "test-id".to_string(),
                tool_name: "bash".to_string(),
                parameters: serde_json::json!({"command": "echo"}),
            },
            preview: None,
        };
        let mut widget = PermissionWidget::new(
            "req-1".to_string(),
            request,
            1,
            super::super::theme::Theme::Dark.palette(),
        );
        widget.preview_text = Text::from(preview.to_string());
        widget.scroll_state = ScrollbarState::new(widget.preview_text.lines.len());
        widget
    }

    fn press(widget: &mut PermissionWidget, code: KeyCode) {
        let action = widget.handle_key_event(KeyEvent::new(code, KeyModifiers::NONE));
        assert!(matches!(action, PermissionModalAction::Nope));
    }

    fn render(widget: &PermissionWidget) -> String {
        render_sized(widget, 24, 12)
    }

    fn render_sized(widget: &PermissionWidget, width: u16, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|f| widget.draw(f, f.area())).unwrap();
        let buffer = terminal.backend().buffer();
        buffer
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>()
    }

    #[test]
    fn left_right_scroll_preview_horizontally() {
        // 20 columns of content in a 16-column pane (24-wide terminal).
        let mut widget = make_widget("0123456789abcdefghij");
        render(&widget);
        assert_eq!(widget.h_scroll_offset, 0);

        press(&mut widget, KeyCode::Left);
        assert_eq!(widget.h_scroll_offset, 0, "cannot scroll before column 0");

        press(&mut widget, KeyCode::Right);
        assert_eq!(widget.h_scroll_offset, 4);

        press(&mut widget, KeyCode::Right);
        assert_eq!(
            widget.h_scroll_offset, 4,
            "stops once the end of the longest line is visible"
        );

        press(&mut widget, KeyCode::Left);
        assert_eq!(widget.h_scroll_offset, 0);
    }

    #[test]
    fn clamp_falls_back_to_last_column_before_first_draw() {
        let mut widget = make_widget("0123456789abcdefghij");
        for _ in 0..20 {
            press(&mut widget, KeyCode::Right);
        }
        assert_eq!(widget.h_scroll_offset, 19);
        // Drawing clamps the offset to the pane width.
        let screen = render(&widget);
        assert!(screen.contains("456789abcdefghij"));
        // Left moves away from the clamped position straight away.
        press(&mut widget, KeyCode::Left);
        assert_eq!(widget.h_scroll_offset, 0);
    }

    #[test]
    fn horizontal_scroll_reveals_cut_off_text() {
        let mut widget = make_widget("0123456789abcdefghijKLMNOPQRST");
        let screen = render(&widget);
        assert!(screen.contains("0123"));
        assert!(!screen.contains("QRST"));

        for _ in 0..10 {
            press(&mut widget, KeyCode::Right);
        }
        assert_eq!(widget.h_scroll_offset, 30 - 16);
        let screen = render(&widget);
        assert!(screen.contains("efghijKLMNOPQRST"));
        assert!(!screen.contains("0123"));
    }

    #[test]
    fn last_column_visible_when_vertical_scrollbar_is_shown() {
        // Ten lines in a four-line pane (24x14 terminal), so the vertical
        // scrollbar takes the rightmost of the 16 columns and text gets 15.
        let mut widget = make_widget("0123456789abcdefghiZ\n2\n3\n4\n5\n6\n7\n8\n9\n10");
        let screen = render_sized(&widget, 24, 14);
        assert!(screen.contains('▲'), "scrollbar is drawn: {screen:?}");
        for _ in 0..10 {
            press(&mut widget, KeyCode::Right);
        }
        assert_eq!(widget.h_scroll_offset, 20 - 15);
        let screen = render_sized(&widget, 24, 14);
        assert!(
            screen.contains("56789abcdefghiZ▲"),
            "last character sits just left of the scrollbar: {screen:?}"
        );
    }

    #[test]
    fn scroll_hint_only_shown_when_content_is_wider_than_pane() {
        let wide = make_widget("0123456789abcdefghijKLMNOPQRST");
        assert!(render(&wide).contains("←/→ scroll"));

        let narrow = make_widget("short line");
        assert!(!render(&narrow).contains("←/→ scroll"));
    }
}
