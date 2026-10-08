pub mod highlight_registry;
pub mod outline;
pub mod registry;

pub use highlight_registry::HighlightRegistry;
pub use outline::{extract_symbols, format_outline, Symbol};
pub use registry::LanguageRegistry;
