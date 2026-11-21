#![allow(clippy::module_inception)]
// async_trait-generated futures trip double_must_use on methods returning Result
#![allow(clippy::double_must_use)]
use clap::{Parser, Subcommand};
use crossterm::{
    cursor,
    event::{self, Event, EventStream, KeyCode, KeyEvent, KeyModifiers},
    terminal::{disable_raw_mode, enable_raw_mode},
    ExecutableCommand,
};
use headless::app::AppHeadless;
use session_options::SessionOptions;

use console::strip_ansi_codes;
use futures::StreamExt;
use openai_dive::v1::resources::chat::{ChatMessage, ChatMessageContent};
use ringbuffer::RingBuffer;
use shai_core::agent::builder::AgentBuilder;
use shai_core::agent::LoggingConfig;
use shai_core::config::agent::AgentConfig;
use shai_core::config::config::ShaiConfig;
use shai_core::runners::clifixer::fix::clifix;
use shai_llm::LlmClient;
use std::env;
use std::io::{self, IsTerminal, Read, Write};
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::{interval, sleep};
use tui::auth::AppAuth;
use tui::theme::logo_cyan;
use tui::App;

#[cfg(unix)]
mod fc;
mod headless;
mod import;
mod session_options;
#[cfg(unix)]
mod shell;

#[cfg(unix)]
use fc::history::CommandHistoryExt;
mod tui;

#[cfg(unix)]
use fc::client::ShaiSessionClient;
#[cfg(unix)]
use shell::pty::ShaiPtyManager;
#[cfg(unix)]
use shell::rc::{get_shell, ShellType};

use crate::headless::tools::list_all_tools;

/// Sentinel value for `--restore` used without a session ID (opens the picker).
const RESTORE_PICKER_SENTINEL: &str = "@picker";

#[derive(Parser)]
#[command(name = "shai")]
#[command(about = "SHAI - Smart terminal wrapper with advanced features")]
#[command(subcommand_required = false)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
    /// Dump entire trace upon completion (headless mode only)
    #[arg(long, global = true)]
    trace: bool,
    /// the url to pull the default shai config
    #[arg(long)]
    default_shai_config_url: Option<String>,
    /// List all available tools
    #[arg(long)]
    list_tools: bool,
    /// Specify which tools to use (comma-separated)
    #[arg(long)]
    tools: Option<String>,
    /// Remove specific tools from the default set (comma-separated)
    #[arg(long)]
    remove: Option<String>,
    /// Set the LLM sampling temperature (default: 0.0)
    #[arg(long)]
    temperature: Option<f32>,
    /// Show version information
    #[arg(short, long)]
    version: bool,
    /// Restore a previous session by session ID.
    /// Without an ID, opens the session picker (TUI only).
    #[arg(short, long, num_args = 0..=1, default_missing_value = "@picker")]
    restore: Option<String>,
    /// Restore the most recent session automatically
    #[arg(long)]
    latest: bool,
    /// Headless mode with prompt
    #[arg(short, long)]
    prompt: Option<String>,
    /// Use specific agent
    #[arg(short, long)]
    agent: Option<String>,
    /// Interactive mode: pipe input then show TUI with context
    #[arg(short, long)]
    interactive: bool,
    /// Hand the session off to another model after the first edit:
    /// a role name or <provider>/<model> (overrides prewalk.into)
    #[arg(long, value_name = "ROLE_OR_MODEL")]
    prewalk_into: Option<String>,
}

#[derive(Subcommand)]
enum Commands {
    #[cfg(unix)]
    /// Start a PTY session with the specified shell
    On {
        /// Shell to run (defaults to $SHELL or /bin/sh)
        #[arg(short, long)]
        shell: Option<ShellType>,
        /// Suppress shell session restoration messages
        #[arg(long, default_value_t = true)]
        quiet: bool,
    },
    #[cfg(unix)]
    /// Exit the current PTY session
    Off,
    #[cfg(unix)]
    /// Is the session on or not
    Status,
    /// Configure SHAI with your AI provider
    Auth,
    /// Agent management commands
    Agent {
        /// Agent name or "list" to list agents
        name: Option<String>,
    },
    /// Session management commands
    Session {
        /// Session ID or "latest" to restore the most recent session
        id: Option<String>,
    },
    /// List agents, sessions, or skills
    List {
        #[command(subcommand)]
        what: Option<ListTarget>,
    },
    #[cfg(unix)]
    /// Send pre-command hook (before command execution)
    #[command(hide = true)]
    Precmd {
        /// The command that is about to be executed
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        command: Vec<String>,
    },
    #[cfg(unix)]
    /// Send post-command hook (analyze last command)
    #[command(hide = true)]
    Postcmd {
        /// Exit code of the last command
        exit_code: i32,
        /// The command that was executed (optional)
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        command: Vec<String>,
    },
    /// Start HTTP server with SSE streaming
    Serve {
        /// Host to bind to (0.0.0.0 for all interfaces, 127.0.0.1 for localhost only)
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
        /// Port to bind to
        #[arg(short, long, default_value = "3000")]
        port: u16,
        /// Agent name to serve (optional)
        agent: Option<String>,
        /// Use ephemeral mode (spawn new agent per request)
        #[arg(long)]
        ephemeral: bool,
        /// Maximum number of concurrent sessions (None = unlimited)
        #[arg(long)]
        max_sessions: Option<usize>,
    },
    /// Import configuration from .claude or .cursor into AGENTS.md
    Import {
        /// Overwrite existing AGENTS.md instead of appending
        #[arg(long)]
        overwrite: bool,
    },
}

#[derive(Subcommand)]
enum ListTarget {
    /// List available agents
    Agent,
    /// List saved sessions
    Session,
    /// List available skills
    Skills,
}

/// Whether this invocation starts an agent session, in the TUI or headless.
fn starts_session(cli: &Cli) -> bool {
    match &cli.command {
        None => !cli.list_tools && !cli.version,
        Some(Commands::Agent { name }) => name.as_deref() != Some("list"),
        Some(Commands::Session { .. }) => true,
        Some(_) => false,
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    let session_starts = starts_session(&cli);
    default_config(cli.default_shai_config_url).await;

    // Session options are resolved only for the commands that start a
    // session, so that the others (auth, list, ...) keep working when an
    // option cannot be resolved, and before anything else those commands do,
    // so that none of their paths runs without them. Asking for one where it
    // cannot apply is an error.
    let session_options = if session_starts {
        SessionOptions::resolve_or_exit(cli.prewalk_into.as_deref())
    } else {
        if cli.prewalk_into.is_some() {
            eprintln!("error: --prewalk-into only applies to commands that start an agent session");
            std::process::exit(2);
        }
        SessionOptions::default()
    };

    match cli.command {
        #[cfg(unix)]
        Some(Commands::On { shell, quiet }) => {
            run_pty(shell, quiet)?;
        }
        #[cfg(unix)]
        Some(Commands::Off) => {
            kill_pty()?;
        }
        #[cfg(unix)]
        Some(Commands::Status) => {
            pty_status()?;
        }
        Some(Commands::Auth) => {
            handle_config().await?;
        }
        Some(Commands::Agent { name }) => match name.as_deref() {
            Some("list") => {
                list_agents();
            }
            Some(name) => {
                handle_main(Some(name.to_string()), None, None, session_options.clone()).await?;
            }
            None => {
                handle_main(
                    None,
                    None,
                    Some(tui::app::InitialModal::AgentPicker),
                    session_options.clone(),
                )
                .await?;
            }
        },
        Some(Commands::Session { id }) => match id.as_deref() {
            Some("latest") => {
                let restore_id = match shai_core::session::SessionPersist::list_sessions() {
                    Ok(sessions) if !sessions.is_empty() => Some(sessions[0].session_id.clone()),
                    _ => {
                        eprintln!("No previous session found.");
                        return Ok(());
                    }
                };
                handle_main(None, restore_id, None, session_options.clone()).await?;
            }
            Some(id) => {
                handle_main(None, Some(id.to_string()), None, session_options.clone()).await?;
            }
            None => {
                handle_main(
                    None,
                    None,
                    Some(tui::app::InitialModal::SessionPicker),
                    session_options.clone(),
                )
                .await?;
            }
        },
        Some(Commands::List { what }) => match what {
            Some(ListTarget::Agent) => list_agents(),
            Some(ListTarget::Session) => list_sessions(),
            Some(ListTarget::Skills) => list_skills(),
            None => {
                list_agents();
                println!();
                list_sessions();
                println!();
                list_skills();
            }
        },
        #[cfg(unix)]
        Some(Commands::Precmd { command }) => {
            let command_str = command.join(" ");
            handle_precmd(command_str)?;
        }
        #[cfg(unix)]
        Some(Commands::Postcmd { exit_code, command }) => {
            let command_str = command.join(" ");
            handle_postcmd(exit_code, command_str).await?;
        }
        Some(Commands::Serve {
            host,
            port,
            agent,
            ephemeral,
            max_sessions,
        }) => {
            handle_serve(host, port, agent, ephemeral, max_sessions).await?;
        }
        Some(Commands::Import { overwrite }) => {
            handle_import(overwrite)?;
        }
        None => {
            // Check for stdin input
            let stdin_input = if !io::stdin().is_terminal() {
                let mut buffer = String::new();
                io::stdin().read_to_string(&mut buffer)?;
                Some(buffer.trim().to_string()).filter(|s| !s.is_empty())
            } else {
                None
            };

            // Handle --list-tools flag
            if cli.list_tools {
                list_all_tools();
                return Ok(());
            }

            // Handle --version flag
            if cli.version {
                show_version()?;
                return Ok(());
            }

            // Handle --prompt flag (headless mode)
            if let Some(prompt) = cli.prompt {
                if cli.restore.as_deref() == Some(RESTORE_PICKER_SENTINEL) {
                    eprintln!(
                        "--restore without a session ID requires the TUI. \
                         Use --restore <id>, --latest, or `shai session`."
                    );
                    return Ok(());
                }
                let mut messages = vec![prompt];
                if let Some(ref stdin_content) = stdin_input {
                    messages.push(stdin_content.clone());
                }
                let _ = handle_fix(HeadlessRun {
                    prompt: messages,
                    tools: cli.tools,
                    remove: cli.remove,
                    trace: cli.trace,
                    agent_name: cli.agent,
                    temperature: cli.temperature,
                    session_options: session_options.clone(),
                })
                .await;
                return Ok(());
            }

            // Handle piped stdin without --prompt
            if let Some(stdin_content) = stdin_input {
                if cli.restore.as_deref() == Some(RESTORE_PICKER_SENTINEL) {
                    eprintln!(
                        "--restore without a session ID requires the TUI. \
                         Use --restore <id>, --latest, or `shai session`."
                    );
                    return Ok(());
                }
                if cli.interactive {
                    // Interactive mode: show TUI with piped content as initial prompt
                    handle_main_with_prompt(
                        cli.agent.clone(),
                        stdin_content,
                        session_options.clone(),
                    )
                    .await?;
                    return Ok(());
                } else {
                    // Headless mode from pipe
                    let _ = handle_fix(HeadlessRun {
                        prompt: vec![stdin_content],
                        tools: cli.tools,
                        remove: cli.remove,
                        trace: cli.trace,
                        agent_name: cli.agent,
                        temperature: cli.temperature,
                        session_options: session_options.clone(),
                    })
                    .await;
                    return Ok(());
                }
            }

            // No input, show TUI
            if cli.restore.as_deref() == Some(RESTORE_PICKER_SENTINEL) {
                // Bare `--restore`: open the session picker
                handle_main(
                    cli.agent.clone(),
                    None,
                    Some(tui::app::InitialModal::SessionPicker),
                    session_options.clone(),
                )
                .await?;
                return Ok(());
            }
            let restore_id = if cli.latest {
                match shai_core::session::SessionPersist::list_sessions() {
                    Ok(sessions) if !sessions.is_empty() => Some(sessions[0].session_id.clone()),
                    _ => {
                        eprintln!("No previous session found.");
                        return Ok(());
                    }
                }
            } else {
                cli.restore.clone()
            };
            handle_main(cli.agent.clone(), restore_id, None, session_options.clone()).await?;
        }
    }

    Ok(())
}

async fn default_config(default_config_url: Option<String>) {
    // One look at the config decides. Looking twice could see it broken and
    // then fine, while another process writes it, and fetch a starter config
    // over it.
    match ShaiConfig::load_for_roles() {
        Ok(_) => return,
        // A config file that is there and does not load, the legacy one
        // included, is the user's to fix: it is neither replaced nor shadowed
        // by a starter config. The warning says where it is broken and not
        // what it holds: the parser's own message can quote a value, and the
        // file holds credentials.
        Err(shai_core::config::roles::RoleError::ConfigInvalid { path, reason }) => {
            eprintln!("Warning: failed to load config at {}: {}", path, reason);
            return;
        }
        Err(_) => {}
    }

    let default_url = match default_config_url {
        Some(url) => url,
        None => {
            "https://raw.githubusercontent.com/ovh/shai/refs/heads/main/.shai.config".to_string()
        }
    };

    let config = if let Ok(parsed_url) = default_url.parse() {
        ShaiConfig::pull_from_url(parsed_url)
            .await
            .unwrap_or_else(|_| ShaiConfig::default())
    } else {
        ShaiConfig::default()
    };

    let _ = config.save();
}

async fn handle_main(
    agent_name: Option<String>,
    restore_session_id: Option<String>,
    initial_modal: Option<tui::app::InitialModal>,
    session_options: SessionOptions,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut app = App::new();
    app.session_options = session_options;
    if let Some(modal) = initial_modal {
        app.initial_modal = modal;
    }
    if let Err(e) = app.run(agent_name, restore_session_id).await {
        eprintln!("error: {}", e)
    }
    Ok(())
}

async fn handle_main_with_prompt(
    agent_name: Option<String>,
    prompt: String,
    session_options: SessionOptions,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut app = App::new();
    app.session_options = session_options;
    app.initial_prompt = Some(prompt);
    if let Err(e) = app.run(agent_name, None).await {
        eprintln!("error: {}", e)
    }
    Ok(())
}

async fn handle_config() -> Result<(), Box<dyn std::error::Error>> {
    let mut auth = AppAuth::new();
    auth.run().await;

    // Standalone entry point: the auth view enabled raw mode and hid the
    // cursor, restore both before handing the terminal back to the shell.
    // (When the auth view runs embedded via /auth, the main TUI owns those.)
    let _ = io::stdout().execute(cursor::Show);
    let _ = disable_raw_mode();
    Ok(())
}

/// Everything a headless run (`-p` or piped stdin) needs from the command line.
struct HeadlessRun {
    prompt: Vec<String>,
    tools: Option<String>,
    remove: Option<String>,
    trace: bool,
    agent_name: Option<String>,
    temperature: Option<f32>,
    session_options: SessionOptions,
}

async fn handle_fix(run: HeadlessRun) -> Result<(), Box<dyn std::error::Error>> {
    let HeadlessRun {
        prompt,
        tools,
        remove,
        trace,
        agent_name,
        temperature,
        session_options,
    } = run;
    let initial_trace: Vec<ChatMessage> = prompt
        .into_iter()
        .map(|p| ChatMessage::User {
            content: ChatMessageContent::Text(p),
            name: None,
        })
        .collect();

    let mut app = AppHeadless::new();
    if let Some(temp) = temperature {
        app.set_temperature(temp);
    }
    app.set_session_options(session_options);
    app.run(initial_trace, tools, remove, trace, agent_name)
        .await
}

fn show_version() -> Result<(), Box<dyn std::error::Error>> {
    println!(
        "{} version {}",
        env!("CARGO_PKG_NAME"),
        env!("CARGO_PKG_VERSION")
    );
    Ok(())
}

#[cfg(unix)]
fn run_pty(shell: Option<ShellType>, quiet: bool) -> Result<(), Box<dyn std::error::Error>> {
    if env::var("SHAI_SESSION_ID").is_ok() {
        eprintln!("Already inside a SHAI session");
        return Ok(());
    }

    let mut pty = ShaiPtyManager::new()?;
    let shell = get_shell(shell)?;
    pty.start_session(shell, quiet)?;
    Ok(())
}

#[cfg(unix)]
fn kill_pty() -> Result<(), Box<dyn std::error::Error>> {
    if env::var("SHAI_SESSION_ID").is_err() {
        eprintln!("Not currently inside a SHAI session");
        return Ok(());
    }

    let ppid = unsafe { libc::getppid() };
    unsafe {
        libc::kill(ppid, libc::SIGHUP);
    }
    std::process::exit(0);
}

#[cfg(unix)]
fn pty_status() -> Result<(), Box<dyn std::error::Error>> {
    if env::var("SHAI_SESSION_ID").is_ok() {
        eprintln!("shAI is enabled");
    } else {
        eprintln!("shAI is disabled");
    }
    Ok(())
}

#[cfg(unix)]
pub fn handle_precmd(command: String) -> Result<(), Box<dyn std::error::Error>> {
    env::var("SHAI_SESSION_ID").ok().and_then(|session_id| {
        let client = ShaiSessionClient::new(&session_id);
        client
            .session_exists()
            .then(|| client.pre_command(&command))
    });
    Ok(())
}

#[cfg(unix)]
pub async fn handle_postcmd(
    exit_code: i32,
    command: String,
) -> Result<(), Box<dyn std::error::Error>> {
    env::var("SHAI_SESSION_ID").ok().and_then(|session_id| {
        let client = ShaiSessionClient::new(&session_id);
        client
            .session_exists()
            .then(|| client.post_command(exit_code, &command))
    });

    match exit_code {
        0 => {
            return Ok(());
        }
        code if code >= 128 => {
            return Ok(());
        }
        _ => {
            let last_terminal_output = env::var("SHAI_SESSION_ID").ok().and_then(|session_id| {
                let client = ShaiSessionClient::new(&session_id);
                client.session_exists().then(|| {
                    client
                        .get_last_commands(50)
                        .unwrap_or_else(|_| vec![].into())
                })
            });

            if let Some(cmd) = last_terminal_output {
                let trace = vec![ChatMessage::User {
                    content: ChatMessageContent::Text(cmd.export_as_text()),
                    name: None,
                }];

                let (llm, model) = ShaiConfig::get_llm().await?;

                enable_raw_mode().unwrap();
                let mut events = EventStream::new();
                let mut ticker = interval(Duration::from_millis(100));
                let spinner_chars = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
                let mut spinner_index = 0;

                let mut clifix_task =
                    tokio::spawn(async move { clifix(Arc::new(llm), model, trace).await });

                let result = loop {
                    tokio::select! {
                        result = &mut clifix_task => {
                            break result.unwrap();
                        }

                        maybe_event = events.next() => {
                            if let Some(Ok(Event::Key(KeyEvent { code: KeyCode::Esc, .. }))) = maybe_event {
                                clifix_task.abort();
                                disable_raw_mode().unwrap();
                                eprintln!("\r\x1b[2K\x1b[2mCancelled.\x1b[0m");
                                return Ok(());
                            }
                        }

                        _ = ticker.tick() => {
                            eprint!("\r\x1b[2mAnalyzing command... {} (Press ESC to cancel)\x1b[0m", spinner_chars[spinner_index]);
                            io::stdout().flush().unwrap();
                            spinner_index = (spinner_index + 1) % spinner_chars.len();
                        }
                    }
                };

                disable_raw_mode().unwrap();
                eprint!("\r\x1b[2K");

                if let Ok(res) = result {
                    if let Some(rational) = &res.short_rational {
                        eprintln!("\n\x1b[2m{}\x1b[0m\n", rational);
                    }
                    eprintln!("\x1b[38;5;206m❯\x1b[0m \x1b[1m{}\x1b[0m", res.fixed_cli);
                    eprintln!("\n\x1b[2m ↵ Run • Esc / Ctrl+C Cancel\x1b[0m");

                    io::stdout().execute(cursor::MoveUp(3)).unwrap();
                    io::stdout()
                        .execute(cursor::MoveToColumn((res.fixed_cli.len() + 3) as u16))
                        .unwrap();
                    io::stdout().flush().unwrap();
                    enable_raw_mode().unwrap();

                    loop {
                        if let Ok(Event::Key(KeyEvent {
                            code, modifiers, ..
                        })) = event::read()
                        {
                            match (code, modifiers) {
                                (KeyCode::Enter, _) => {
                                    disable_raw_mode().unwrap();
                                    io::stdout().execute(cursor::MoveDown(3)).unwrap();
                                    io::stdout().execute(cursor::MoveToColumn(0)).unwrap();
                                    println!();

                                    let mut cmd = Command::new("sh");
                                    cmd.arg("-c").arg(&res.fixed_cli);
                                    cmd.envs(env::vars());

                                    match cmd.status() {
                                        Ok(status) => {
                                            if status.success() {
                                                shell::rc::write_to_shell_history(&res.fixed_cli);
                                            }
                                        }
                                        Err(e) => eprintln!("Failed to execute command: {}\n", e),
                                    }
                                    break;
                                }
                                (KeyCode::Esc, _) => {
                                    disable_raw_mode().unwrap();
                                    println!();
                                    break;
                                }
                                (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                                    disable_raw_mode().unwrap();
                                    println!();
                                    eprintln!("Exiting...");
                                    std::process::exit(0);
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(())
}

fn handle_import(overwrite: bool) -> Result<(), Box<dyn std::error::Error>> {
    let base = std::env::current_dir()?;
    match import::import_to_agents_md(&base, overwrite) {
        Ok((path, count)) => {
            println!(
                "\x1b[32m✓\x1b[0m Imported {} config(s) into {}",
                count,
                path.display()
            );
            Ok(())
        }
        Err(e) => {
            eprintln!("\x1b[31m✗\x1b[0m {}", e);
            std::process::exit(1);
        }
    }
}

async fn handle_serve(
    host: String,
    port: u16,
    // TODO: wire `--agent` to ServerConfig so the HTTP server uses it as default agent name
    _agent: Option<String>,
    ephemeral: bool,
    max_sessions: Option<usize>,
) -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing for HTTP server logs
    tracing_subscriber::fmt()
        .with_target(false)
        .with_level(true)
        .with_env_filter("shai_http=debug")
        .init();

    println!("{}", logo_cyan());

    let addr = format!("{}:{}", host, port);
    let config = shai_http::ServerConfig::new(addr)
        .with_ephemeral(ephemeral)
        .with_max_sessions(max_sessions);

    shai_http::start_server(config).await?;

    Ok(())
}

fn list_agents() {
    let agents = match AgentConfig::list_agents() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("Failed to list agents: {}", e);
            return;
        }
    };

    if agents.is_empty() {
        println!("No custom agents found.");
        println!("Create agent configs in ~/.config/shai/agents/");
        return;
    }

    println!("Available agents:");
    let max_name_len = agents.iter().map(|name| name.len()).max().unwrap_or(0);

    for agent in agents {
        match AgentConfig::load(&agent) {
            Ok(config) => {
                println!(
                    "  \x1b[1m{:<width$}\x1b[0m \x1b[2m{}\x1b[0m",
                    agent,
                    config.description,
                    width = max_name_len
                );
            }
            Err(e) => {
                eprintln!(
                    "  \x1b[1m{:<width$}\x1b[0m \x1b[2m(config error: {})\x1b[0m",
                    agent,
                    e,
                    width = max_name_len
                );
            }
        }
    }
}

fn list_sessions() {
    match shai_core::session::SessionPersist::list_sessions() {
        Ok(sessions) => {
            if sessions.is_empty() {
                println!("No saved sessions found.");
                return;
            }

            println!("Saved sessions:");
            for session in &sessions {
                let id_short = &session.session_id[..8.min(session.session_id.len())];
                let name = session.name.as_deref().unwrap_or("unnamed");
                println!("  \x1b[1m{:<20}\x1b[0m \x1b[2m({})\x1b[0m", name, id_short);
            }
        }
        Err(e) => {
            eprintln!("Failed to list sessions: {}", e);
        }
    }
}

fn list_skills() {
    let skills = shai_core::tools::skills::discovery::discover_skills();
    if skills.is_empty() {
        println!("No skills found.");
        return;
    }

    println!("Available skills:");
    for skill in &skills {
        if skill.description.is_empty() {
            println!(
                "  \x1b[36m\u{2022}\x1b[0m {} \x1b[90m({})\x1b[0m",
                skill.name,
                skill.source()
            );
        } else {
            println!(
                "  \x1b[36m\u{2022}\x1b[0m \x1b[1m{}\x1b[0m \u{2014} {} \x1b[90m({})\x1b[0m",
                skill.name,
                skill.description,
                skill.source()
            );
        }
    }
}

#[cfg(test)]
mod cli_tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn cli_definition_is_consistent() {
        Cli::command().debug_assert();
    }

    #[test]
    fn session_options_only_apply_where_a_session_starts() {
        for args in [
            &["shai"][..],
            &["shai", "-p", "do it"],
            &["shai", "--latest"],
            &["shai", "agent"],
            &["shai", "agent", "reviewer"],
            &["shai", "session"],
            &["shai", "session", "latest"],
        ] {
            let cli = Cli::try_parse_from(args).unwrap();
            assert!(starts_session(&cli), "{args:?}");
        }
        for args in [
            &["shai", "--list-tools"][..],
            &["shai", "--version"],
            &["shai", "agent", "list"],
            &["shai", "auth"],
            &["shai", "import"],
        ] {
            let cli = Cli::try_parse_from(args).unwrap();
            assert!(!starts_session(&cli), "{args:?}");
        }
    }

    #[test]
    fn prewalk_is_off_unless_asked_for() {
        let cli = Cli::try_parse_from(["shai"]).unwrap();
        assert!(cli.prewalk_into.is_none());
        let cli = Cli::try_parse_from(["shai", "-p", "do it"]).unwrap();
        assert!(cli.prewalk_into.is_none());
    }

    #[test]
    fn prewalk_into_takes_a_role_or_a_model_reference() {
        let cli = Cli::try_parse_from(["shai", "--prewalk-into", "cheap"]).unwrap();
        assert_eq!(cli.prewalk_into.as_deref(), Some("cheap"));

        let cli =
            Cli::try_parse_from(["shai", "-p", "do it", "--prewalk-into=ollama/qwen3:8b"]).unwrap();
        assert_eq!(cli.prewalk_into.as_deref(), Some("ollama/qwen3:8b"));
        assert_eq!(cli.prompt.as_deref(), Some("do it"));

        // The flag needs a value.
        assert!(Cli::try_parse_from(["shai", "--prewalk-into"]).is_err());
    }
}
