pub mod ansi;
pub mod bash_aware;
pub mod compact;
pub mod generic;
pub mod trace;

pub use compact::compact_tool_result;
pub use trace::compact_trace_if_needed;
