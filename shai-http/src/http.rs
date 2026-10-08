use axum::{
    routing::{get, post},
    Router,
};
use std::sync::Arc;
use tower_http::cors::CorsLayer;
use tracing::info;

use crate::apis;
use crate::session::{SessionManager, SessionManagerConfig};

/// Configuration for the HTTP server
#[derive(Clone, Debug)]
pub struct ServerConfig {
    /// Server bind address (e.g., "127.0.0.1:8080")
    pub address: String,
    /// Session manager configuration
    pub session_manager: SessionManagerConfig,
}

impl ServerConfig {
    /// Create a new server config with the given address and default session manager config
    pub fn new(address: String) -> Self {
        Self {
            address,
            session_manager: SessionManagerConfig::default(),
        }
    }

    /// Set whether sessions are ephemeral by default
    pub fn with_ephemeral(mut self, ephemeral: bool) -> Self {
        self.session_manager.ephemeral = ephemeral;
        self
    }

    /// Set the maximum number of concurrent sessions
    pub fn with_max_sessions(mut self, max_sessions: Option<usize>) -> Self {
        self.session_manager.max_sessions = max_sessions;
        self
    }
}

/// Server state holding the session manager
#[derive(Clone)]
pub struct ServerState {
    pub session_manager: Arc<SessionManager>,
}

/// Start the HTTP server with SSE streaming
pub async fn start_server(config: ServerConfig) -> Result<(), Box<dyn std::error::Error>> {
    // Create session manager
    let session_manager = SessionManager::new(config.session_manager.clone());

    println!("✓ Session manager initialized");
    if let Some(max) = config.session_manager.max_sessions {
        println!("  Max sessions: \x1b[1m{}\x1b[0m", max);
    } else {
        println!("  Max sessions: \x1b[1munlimited\x1b[0m");
    }
    println!(
        "  Default mode: \x1b[1m{}\x1b[0m",
        if config.session_manager.ephemeral {
            "ephemeral"
        } else {
            "persistent"
        }
    );
    println!();

    let state = ServerState {
        session_manager: Arc::new(session_manager),
    };

    let app = Router::new()
        // Simple API
        .route(
            "/v1/multimodal",
            post(apis::simple::handle_multimodal_query_stream),
        )
        .route(
            "/v1/multimodal/{session_id}",
            post(apis::simple::handle_multimodal_query_stream_with_session),
        )
        // OpenAI-compatible Response API
        .route("/v1/responses", post(apis::openai::handle_response))
        .route(
            "/v1/responses/{response_id}",
            get(apis::openai::handle_get_response),
        )
        .route(
            "/v1/responses/{response_id}/cancel",
            post(apis::openai::handle_cancel_response),
        )
        // OpenAI-compatible Chat Completion API
        .route(
            "/v1/chat/completions",
            post(apis::openai::handle_chat_completion),
        )
        .layer(CorsLayer::permissive())
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(&config.address).await?;

    // Print server info
    println!("Server starting on \x1b[1mhttp://{}\x1b[0m", config.address);
    println!("\nAvailable endpoints:");
    println!("  \x1b[1mPOST /v1/chat/completions\x1b[0m            - OpenAI Chat Completions API (ephemeral)");
    println!("  \x1b[1mPOST /v1/responses\x1b[0m                    - OpenAI Responses API (stateful/stateless)");
    println!("  \x1b[1mGET  /v1/responses/:id\x1b[0m                - Get response by ID");
    println!("  \x1b[1mPOST /v1/responses/:id/cancel\x1b[0m        - Cancel a response");
    println!(
        "  \x1b[1mPOST /v1/multimodal\x1b[0m                   - Simple multimodal API (streaming)"
    );
    println!("  \x1b[1mPOST /v1/multimodal/:session_id\x1b[0m      - Simple multimodal API (with session)");

    // List available agents
    use shai_core::config::agent::AgentConfig;
    match AgentConfig::list_agents() {
        Ok(agents) if !agents.is_empty() => {
            println!("\nAvailable agents: \x1b[2m{}\x1b[0m", agents.join(", "));
        }
        _ => {}
    }

    println!("\nPress Ctrl+C to stop\n");

    info!("HTTP server listening on {}", config.address);

    axum::serve(listener, app).await?;
    Ok(())
}
