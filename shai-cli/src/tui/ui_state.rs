use super::perm::PermissionWidget;

pub enum AppModalState<'a> {
    InputShown,
    PermissionModal { widget: PermissionWidget<'a> },
}

pub struct UiState<'a> {
    pub modal_state: AppModalState<'a>,
    pub session_picker: Option<super::session_picker::SessionPicker>,
    pub agent_picker: Option<super::agent_picker::AgentPicker>,
    pub exit: bool,
}

impl<'a> UiState<'a> {
    pub fn new() -> Self {
        Self {
            modal_state: AppModalState::InputShown,
            session_picker: None,
            agent_picker: None,
            exit: false,
        }
    }
}

impl<'a> Default for UiState<'a> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_initializes_defaults() {
        let state = UiState::new();
        assert!(state.session_picker.is_none());
        assert!(!state.exit);
        match state.modal_state {
            AppModalState::InputShown => {}
            _ => panic!("expected InputShown variant"),
        }
    }
}
