use shai_core::tools::McpServerStatus;

pub struct McpManager {
    servers: Vec<McpServerStatus>,
}

impl McpManager {
    pub fn new() -> Self {
        Self {
            servers: Vec::new(),
        }
    }

    pub fn set_servers(&mut self, servers: Vec<McpServerStatus>) {
        self.servers = servers;
    }

    pub fn is_empty(&self) -> bool {
        self.servers.is_empty()
    }
}

impl std::fmt::Display for McpManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.servers.is_empty() {
            return write!(f, "No MCP servers configured.");
        }

        writeln!(f, "MCP Servers:")?;
        for server in &self.servers {
            let status = if server.connected {
                format!("\u{2713} connected ({} tools)", server.tool_count)
            } else {
                format!("\u{2717} failed{}", {
                    if let Some(ref err) = server.error {
                        format!(": {}", err)
                    } else {
                        String::new()
                    }
                })
            };
            writeln!(f, "  {} {}", server.name, status)?;
        }
        Ok(())
    }
}
