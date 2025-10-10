pub mod tool;
pub mod call;
pub mod call_fc_auto;
pub mod call_fc_required;
pub mod call_structured_output;

#[cfg(test)]
mod test_so;

pub use tool::{ToolDescription, ToolCallMethod, ToolBox, ContainsTool};
pub use call::{LlmToolCall,ToolCallAuto};
pub use call_structured_output::{AssistantResponse, StructuredOutputBuilder, IntoChatMessage};
pub use call_fc_auto::FunctionCallingAutoBuilder;
pub use call_fc_required::FunctionCallingRequiredBuilder;

pub mod max_context;
pub use max_context::get_max_context;