use std::os::unix::net::UnixStream;
use std::path::Path;

use crate::fc::history::{CommandEntry, CommandHistory, HistoryStats};
use crate::fc::protocol::{ShaiProtocol, ShaiRequest, ShaiResponse, ResponseData};

/// Client for querying the command history via Unix socket
pub struct ShaiSessionClient {
    socket_path: String,
}

impl ShaiSessionClient {
    pub fn new(session_id: &str) -> Self {
        let socket_path = format!("/tmp/shai_history_{}", session_id);
        Self { socket_path }
    }

    pub fn get_last_commands(&self, n: usize) -> Result<CommandHistory, Box<dyn std::error::Error>> {
        let mut stream = UnixStream::connect(&self.socket_path)
            .map_err(|_| "Could not connect to SHAI history session (is server running?)")?;
        
        let request = ShaiRequest::GetLastCmd { n };
        ShaiProtocol::write_request(&mut stream, &request)?;
        
        let response = ShaiProtocol::read_response(&mut stream)?;
        
        match response {
            ShaiResponse::Ok { data: ResponseData::Commands(entries) } => {
                // Handle empty vec to avoid creating buffer with capacity 0
                if entries.is_empty() {
                    Ok(CommandHistory::new(1))
                } else {
                    Ok(entries.into())
                }
            },
            ShaiResponse::Ok { .. } => Err("Unexpected response type".into()),
            ShaiResponse::Error { message } => Err(message.into()),
        }
    }

    pub fn get_all_commands(&self) -> Result<CommandHistory, Box<dyn std::error::Error>> {
        let mut stream = UnixStream::connect(&self.socket_path)
            .map_err(|_| "Could not connect to SHAI history session (is server running?)")?;
        
        let request = ShaiRequest::GetAllCmd;
        ShaiProtocol::write_request(&mut stream, &request)?;
        
        let response = ShaiProtocol::read_response(&mut stream)?;
        
        match response {
            ShaiResponse::Ok { data: ResponseData::Commands(entries) } => {
                // Handle empty vec to avoid creating buffer with capacity 0
                if entries.is_empty() {
                    Ok(CommandHistory::new(1))
                } else {
                    Ok(entries.into())
                }
            },
            ShaiResponse::Ok { .. } => Err("Unexpected response type".into()),
            ShaiResponse::Error { message } => Err(message.into()),
        }
    }

    pub fn clear(&self) -> Result<(), Box<dyn std::error::Error>> {
        let mut stream = UnixStream::connect(&self.socket_path)
            .map_err(|_| "Could not connect to SHAI history session (is server running?)")?;
        
        let request = ShaiRequest::Clear;
        ShaiProtocol::write_request(&mut stream, &request)?;
        
        let response = ShaiProtocol::read_response(&mut stream)?;
        
        match response {
            ShaiResponse::Ok { .. } => Ok(()),
            ShaiResponse::Error { message } => Err(message.into()),
        }
    }

    pub fn get_status(&self) -> Result<HistoryStats, Box<dyn std::error::Error>> {
        let mut stream = UnixStream::connect(&self.socket_path)
            .map_err(|_| "Could not connect to SHAI history session (is server running?)")?;
        
        let request = ShaiRequest::Status;
        ShaiProtocol::write_request(&mut stream, &request)?;
        
        let response = ShaiProtocol::read_response(&mut stream)?;
        
        match response {
            ShaiResponse::Ok { data: ResponseData::Stats(stats) } => Ok(stats),
            ShaiResponse::Ok { .. } => Err("Unexpected response type".into()),
            ShaiResponse::Error { message } => Err(message.into()),
        }
    }

    pub fn pre_command(&self, cmd: &str) -> Result<(), Box<dyn std::error::Error>> {
        let mut stream = UnixStream::connect(&self.socket_path)
            .map_err(|_| "Could not connect to SHAI history session (is server running?)")?;
        
        let request = ShaiRequest::PreCmd { cmd: cmd.to_string() };
        ShaiProtocol::write_request(&mut stream, &request)?;
        
        let response = ShaiProtocol::read_response(&mut stream)?;
        
        match response {
            ShaiResponse::Ok { .. } => Ok(()),
            ShaiResponse::Error { message } => Err(message.into()),
        }
    }

    pub fn post_command(&self, exit_code: i32,  cmd: &str) -> Result<(), Box<dyn std::error::Error>> {
        let mut stream = UnixStream::connect(&self.socket_path)
            .map_err(|_| "Could not connect to SHAI history session (is server running?)")?;
        
        let request = ShaiRequest::PostCmd { 
            cmd: cmd.to_string(), 
            exit_code
        };
        ShaiProtocol::write_request(&mut stream, &request)?;
        
        let response = ShaiProtocol::read_response(&mut stream)?;
        
        match response {
            ShaiResponse::Ok { .. } => Ok(()),
            ShaiResponse::Error { message } => Err(message.into()),
        }
    }

    pub fn session_exists(&self) -> bool {
        Path::new(&self.socket_path).exists()
    }
}

