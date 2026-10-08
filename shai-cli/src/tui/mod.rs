pub mod agent_meta;
pub mod agent_picker;
pub mod agent_state;
pub mod app;
pub mod auth;
pub mod command;
pub mod handler;
pub mod helper;
pub mod history;
pub mod input;
pub mod key_handler;
pub mod mcp_manager;
pub mod modal;
pub mod perm;
pub mod perm_alt_screen;
pub mod perm_manager;
pub mod prompt_picker;
pub mod renderer;
pub mod session_manager;
pub mod session_picker;
pub mod shortcuts;
pub mod statusbar;
pub mod suggestion;
pub mod theme;
pub mod token_counter;
pub mod tool_tracker;
pub mod ui_state;
pub mod viewer;

pub use app::App;

#[cfg(test)]
pub mod test_utils {
    use shai_core::tools::ToolCall;

    pub fn make_tool_call(id: &str, name: &str) -> ToolCall {
        ToolCall {
            tool_call_id: id.to_string(),
            tool_name: name.to_string(),
            parameters: serde_json::json!({"path": "/tmp/test.txt"}),
        }
    }
}
