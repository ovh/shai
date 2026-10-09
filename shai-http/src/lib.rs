#![allow(clippy::module_inception)]
// async_trait-generated futures trip double_must_use on methods returning Result
#![allow(clippy::double_must_use)]
pub mod apis;
pub mod error;
pub mod http;
pub mod session;
pub mod streaming;

pub use error::{ApiJson, ErrorResponse};
pub use http::{start_server, ServerConfig, ServerState};
pub use session::{AgentSession, SessionManager, SessionManagerConfig};
pub use streaming::{event_to_sse_stream, session_to_sse_stream, EventFormatter};
