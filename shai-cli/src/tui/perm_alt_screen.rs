use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::LeaveAlternateScreen;
use ratatui::{layout::Rect, Frame};
use std::io::{self, stdout, Write};

use super::perm::{PermissionModalAction, PermissionWidget};
use super::theme::ThemePalette;

pub struct AlternateScreenPermissionModal<'a> {
    widget: PermissionWidget<'a>,
    request_id: String,
}

impl AlternateScreenPermissionModal<'_> {
    pub fn new(widget: &PermissionWidget, palette: ThemePalette) -> io::Result<Self> {
        let request_id = widget.request_id.clone();
        let widget = PermissionWidget::new(
            request_id.clone(),
            widget.request.clone(),
            widget.remaining_perms,
            palette,
        );
        Ok(Self { widget, request_id })
    }

    pub async fn run(&mut self) -> io::Result<PermissionModalAction> {
        crate::tui::modal::run_alternate_screen(self).await
    }
}

impl crate::tui::modal::Modal for AlternateScreenPermissionModal<'_> {
    type Output = PermissionModalAction;

    fn draw(&mut self, frame: &mut Frame, area: Rect) {
        self.widget.draw(frame, area);
    }

    fn handle_key_event(&mut self, key_event: crossterm::event::KeyEvent) -> Option<Self::Output> {
        if key_event.code == KeyCode::Char('c')
            && key_event.modifiers.contains(KeyModifiers::CONTROL)
        {
            return Some(PermissionModalAction::Response {
                request_id: self.request_id.clone(),
                choice: shai_core::agent::PermissionResponse::Deny,
            });
        }

        let action = self.widget.handle_key_event(key_event);
        if !matches!(action, PermissionModalAction::Nope) {
            Some(action)
        } else {
            None
        }
    }
}

impl Drop for AlternateScreenPermissionModal<'_> {
    fn drop(&mut self) {
        let _ = execute!(stdout(), LeaveAlternateScreen);
        let _ = stdout().flush();
    }
}
