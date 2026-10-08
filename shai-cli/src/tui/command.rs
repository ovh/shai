use shai_llm::ToolCallMethod;
use std::{io, time::Duration};

use super::session_picker::SessionPicker;
use super::theme::Theme;
use crate::tui::App;

#[derive(Clone)]
pub struct CommandDef {
    pub name: &'static str,
    pub description: &'static str,
    pub args: &'static [&'static str],
}

const COMMANDS: &[CommandDef] = &[
    CommandDef {
        name: "/exit",
        description: "exit from the tui",
        args: &[],
    },
    CommandDef {
        name: "/tc",
        description: "set the tool call method: [auto | fc | fc2 | so]",
        args: &["method"],
    },
    CommandDef {
        name: "/temp",
        description: "set the sampling temperature (e.g. /temp 0.3)",
        args: &["temperature"],
    },
    CommandDef {
        name: "/tokens",
        description: "display token usage (input/output)",
        args: &[],
    },
    CommandDef {
        name: "/theme",
        description: "set theme: [dark | light | toggle]",
        args: &["mode"],
    },
    CommandDef {
        name: "/restore",
        description: "restore a previous session",
        args: &[],
    },
    CommandDef {
        name: "/session",
        description: "open the session picker",
        args: &[],
    },
    CommandDef {
        name: "/pwd",
        description: "show current working directory",
        args: &[],
    },
    CommandDef {
        name: "/latest",
        description: "restore the most recent session",
        args: &[],
    },
    CommandDef {
        name: "/skills",
        description: "list available skills",
        args: &[],
    },
    CommandDef {
        name: "/regenerate",
        description: "regenerate the last response",
        args: &[],
    },
    CommandDef {
        name: "/tools",
        description: "list all registered tools",
        args: &[],
    },
    CommandDef {
        name: "/mcp",
        description: "list MCP servers and connection status",
        args: &[],
    },
    CommandDef {
        name: "/auth",
        description: "configure AI provider",
        args: &[],
    },
    CommandDef {
        name: "/agent",
        description: "switch agent",
        args: &[],
    },
];

pub struct CommandRegistry;

impl CommandRegistry {
    pub fn commands() -> &'static [CommandDef] {
        COMMANDS
    }

    pub async fn dispatch(command: &str, app: &mut App<'_>) -> io::Result<()> {
        let mut parts = command.split_whitespace();
        let cmd = parts.next().unwrap();
        let args: Vec<&str> = parts.collect();

        match cmd {
            "/exit" => {
                app.ui_state.exit = true;
            }
            "/tc" => {
                if let Some(ref agent) = app.agent {
                    match args.into_iter().next() {
                        Some("auto") => {
                            if let Ok(method) = agent
                                .controller
                                .set_method(Some(ToolCallMethod::Auto))
                                .await
                            {
                                app.notify(
                                    "llm will now try all method for tool calls",
                                    Duration::from_secs(3),
                                );
                                app.input.set_tool_call_method(method);
                                app.status_bar
                                    .set_tool_call_method(app.input.tool_call_method_str());
                            }
                        }
                        Some("fc") => {
                            if let Ok(method) = agent
                                .controller
                                .set_method(Some(ToolCallMethod::FunctionCall))
                                .await
                            {
                                app.notify(
                                    "llm will now use function calling api for tool calls",
                                    Duration::from_secs(3),
                                );
                                app.input.set_tool_call_method(method);
                                app.status_bar
                                    .set_tool_call_method(app.input.tool_call_method_str());
                            }
                        }
                        Some("fc2") => {
                            if let Ok(method) = agent
                                .controller
                                .set_method(Some(ToolCallMethod::FunctionCallRequired))
                                .await
                            {
                                app.notify("llm will now use function calling in required mode for tool calls", Duration::from_secs(3));
                                app.input.set_tool_call_method(method);
                                app.status_bar
                                    .set_tool_call_method(app.input.tool_call_method_str());
                            }
                        }
                        Some("so") => {
                            if let Ok(method) = agent
                                .controller
                                .set_method(Some(ToolCallMethod::StructuredOutput))
                                .await
                            {
                                app.notify(
                                    "llm will now use structured output for tool calls",
                                    Duration::from_secs(3),
                                );
                                app.input.set_tool_call_method(method);
                                app.status_bar
                                    .set_tool_call_method(app.input.tool_call_method_str());
                            }
                        }
                        _ => {}
                    }
                }
            }
            "/regenerate" => {
                if let Some(ref agent) = app.agent {
                    let _ = agent.controller.regenerate().await;
                    app.notify("Regenerating last response...", Duration::from_secs(2));
                }
            }
            "/temp" => {
                if let Some(ref agent) = app.agent {
                    match args.into_iter().next() {
                        Some(temp_str) => match temp_str.parse::<f32>() {
                            Ok(temp) => match agent.controller.set_temperature(temp).await {
                                Ok(temp) => {
                                    app.notify(
                                        &format!("Temperature set to {:.1}", temp),
                                        Duration::from_secs(2),
                                    );
                                }
                                Err(e) => {
                                    app.notify(
                                        &format!("Failed to set temperature: {}", e),
                                        Duration::from_secs(3),
                                    );
                                }
                            },
                            Err(_) => {
                                app.notify("Usage: /temp <float>", Duration::from_secs(3));
                            }
                        },
                        None => {
                            app.notify("Usage: /temp <float>", Duration::from_secs(3));
                        }
                    }
                }
            }
            "/tokens" => {
                let tc = app.agent_state.token_counter();
                let msg = format!(
                    "\x1b[1mToken Usage:\x1b[0m\n  \x1b[36m\u{2022}\x1b[0m Input:    {}\n  \x1b[36m\u{2022}\x1b[0m Output:   {}\n  \x1b[36m\u{2022}\x1b[0m Cached:   {}\n  \x1b[36m\u{2022}\x1b[0m Total:    {}",
                    tc.input_tokens(),
                    tc.output_tokens(),
                    tc.cached_tokens(),
                    tc.total()
                );
                app.renderer.history_mut().add_system_text(&msg);
            }
            "/theme" => match args.into_iter().next() {
                Some("dark") => {
                    *app.status_bar.theme_mut() = Theme::Dark;
                    let new_palette = app.status_bar.palette();
                    app.input.set_palette(new_palette);
                    app.notify("Theme set to dark", Duration::from_secs(2));
                }
                Some("light") => {
                    *app.status_bar.theme_mut() = Theme::Light;
                    let new_palette = app.status_bar.palette();
                    app.input.set_palette(new_palette);
                    app.notify("Theme set to light", Duration::from_secs(2));
                }
                Some("toggle") => {
                    app.status_bar.theme_mut().toggle();
                    let new_palette = app.status_bar.palette();
                    app.input.set_palette(new_palette);
                    let theme_name = match app.status_bar.theme() {
                        Theme::Dark => "dark",
                        Theme::Light => "light",
                    };
                    app.notify(
                        &format!("Theme toggled to {}", theme_name),
                        Duration::from_secs(2),
                    );
                }
                _ => {
                    app.notify("Usage: /theme [dark|light|toggle]", Duration::from_secs(3));
                }
            },
            "/restore" => {
                let sessions = shai_core::session::SessionPersist::list_sessions();
                match sessions {
                    Ok(sessions) if !sessions.is_empty() => {
                        if let Some(arg) = args.into_iter().next() {
                            let selected = arg
                                .parse::<usize>()
                                .ok()
                                .and_then(|idx| {
                                    if idx > 0 && idx <= sessions.len() {
                                        Some(&sessions[idx - 1])
                                    } else {
                                        None
                                    }
                                })
                                .or_else(|| {
                                    sessions.iter().find(|s| s.session_id.starts_with(arg))
                                });

                            if let Some(session) = selected {
                                app.notify(
                                    &format!("Restoring session {}...", &session.session_id[..8]),
                                    Duration::from_secs(2),
                                );

                                if let Some(agent) = app.agent.take() {
                                    let _ = agent.controller.terminate().await;
                                }

                                let agent_name = app.agent_meta.name().map(|s| s.to_string());
                                app.start_agent(agent_name.as_deref()).await.map_err(|e| {
                                    io::Error::other(format!("Failed to start agent: {}", e))
                                })?;

                                if let Some(ref agent) = app.agent {
                                    let _ =
                                        agent.controller.load_trace(session.trace.clone()).await;
                                }

                                app.agent_state
                                    .session_manager_mut()
                                    .set_session_id(&session.session_id);
                                app.render_restored_trace(&session.trace);

                                app.notify(
                                    &format!("Session {} restored", &session.session_id[..8]),
                                    Duration::from_secs(2),
                                );
                            } else {
                                app.notify("Invalid session number", Duration::from_secs(2));
                            }
                        } else {
                            let palette = app.status_bar.palette();
                            app.ui_state.session_picker =
                                Some(SessionPicker::new(sessions.clone(), palette));
                        }
                    }
                    Ok(_) => {
                        app.notify("No saved sessions found", Duration::from_secs(2));
                    }
                    Err(e) => {
                        app.notify(
                            &format!("Failed to list sessions: {}", e),
                            Duration::from_secs(3),
                        );
                    }
                }
            }
            "/session" => {
                let sessions = shai_core::session::SessionPersist::list_sessions();
                match sessions {
                    Ok(sessions) if !sessions.is_empty() => {
                        let palette = app.status_bar.palette();
                        app.ui_state.session_picker =
                            Some(SessionPicker::new(sessions, palette));
                    }
                    Ok(_) => {
                        app.notify("No saved sessions found", Duration::from_secs(2));
                    }
                    Err(e) => {
                        app.notify(
                            &format!("Failed to list sessions: {}", e),
                            Duration::from_secs(3),
                        );
                    }
                }
            }
            "/pwd" => {
                match std::env::current_dir() {
                    Ok(cwd) => {
                        app.renderer
                            .history_mut()
                            .add_system_text(&cwd.display().to_string());
                    }
                    Err(_) => {
                        app.notify("Failed to get current directory", Duration::from_secs(3));
                    }
                }
            }
            "/latest" => {
                match shai_core::session::SessionPersist::list_sessions() {
                    Ok(sessions) if !sessions.is_empty() => {
                        let session = &sessions[0];
                        app.notify(
                            &format!("Restoring session {}...", &session.session_id[..8]),
                            Duration::from_secs(2),
                        );

                        if let Some(agent) = app.agent.take() {
                            let _ = agent.controller.terminate().await;
                        }

                        let agent_name = app.agent_meta.name().map(|s| s.to_string());
                        app.start_agent(agent_name.as_deref()).await.map_err(|e| {
                            io::Error::other(format!("Failed to start agent: {}", e))
                        })?;

                        if let Some(ref agent) = app.agent {
                            let _ = agent.controller.load_trace(session.trace.clone()).await;
                        }

                        app.agent_state
                            .session_manager_mut()
                            .set_session_id(&session.session_id);
                        app.render_restored_trace(&session.trace);

                        app.notify(
                            &format!("Session {} restored", &session.session_id[..8]),
                            Duration::from_secs(2),
                        );
                    }
                    Ok(_) => {
                        app.notify("No saved sessions found", Duration::from_secs(2));
                    }
                    Err(e) => {
                        app.notify(
                            &format!("Failed to list sessions: {}", e),
                            Duration::from_secs(3),
                        );
                    }
                }
            }
            "/skills" => {
                let skills = shai_core::tools::skills::discovery::discover_skills();
                if skills.is_empty() {
                    app.notify("No skills found.", Duration::from_secs(3));
                } else {
                    let mut msg = String::from("\x1b[1mAvailable skills:\x1b[0m\n");
                    for skill in &skills {
                        if skill.description.is_empty() {
                            msg.push_str(&format!("  \x1b[36m\u{2022}\x1b[0m {}\n", skill.name));
                        } else {
                            msg.push_str(&format!(
                                "  \x1b[36m\u{2022}\x1b[0m \x1b[1m{}\x1b[0m \u{2014} {}\n",
                                skill.name, skill.description
                            ));
                        }
                    }
                    app.renderer.history_mut().add_system_text(&msg);
                }
            }
            "/tools" => {
                let tools = app.agent.as_ref().map(|a| a.tools.clone()).unwrap_or_default();
                if tools.is_empty() {
                    app.notify("No tools available.", Duration::from_secs(2));
                } else {
                    let mut msg = String::from("\x1b[1mAvailable tools:\x1b[0m\n");
                    for (name, desc) in &tools {
                        msg.push_str(&format!("  \x1b[36m\u{2022}\x1b[0m \x1b[1m{}\x1b[0m \u{2014} {}\n", name, desc));
                    }
                    app.renderer.history_mut().add_system_text(&msg);
                }
            }
            "/mcp" => {
                let mcp = app.agent_state.mcp_manager();
                if mcp.is_empty() {
                    app.notify("No MCP servers configured.", Duration::from_secs(3));
                } else {
                    let msg = format!("{}", mcp);
                    app.renderer.history_mut().add_system_text(&msg);
                }
            }
            "/auth" => {
                // Save agent name and trace before reconfiguring
                let agent_name = app.agent_meta.name().map(|s| s.to_string());
                let trace = if let Some(ref agent) = app.agent {
                    agent.controller.get_trace().await.ok()
                } else {
                    None
                };

                // Terminate current agent
                if let Some(agent) = app.agent.take() {
                    let _ = agent.controller.terminate().await;
                    let _ = agent.handle.await;
                }

                // Run auth TUI
                let mut auth = crate::tui::auth::auth::AppAuth::new();
                auth.run().await;

                // Recreate agent
                app.start_agent(agent_name.as_deref()).await.ok();

                if let Some(trace) = trace {
                    if let Some(ref agent) = app.agent {
                        let _ = agent.controller.load_trace(trace).await;
                    }
                }

                app.notify("Provider configuration updated", Duration::from_secs(2));
            }
            "/agent" => {
                app.ui_state.agent_picker =
                    Some(super::agent_picker::AgentPicker::new(app.status_bar.palette()));
            }
            _ => {
                app.notify("command unknown", Duration::from_secs(1));
            }
        }
        Ok(())
    }
}
