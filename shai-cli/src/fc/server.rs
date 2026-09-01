use std::os::unix::net::{UnixListener, UnixStream};
use std::thread;
use std::sync::{Arc, Mutex};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use ringbuffer::RingBuffer;
use crate::fc::history::{CommandEntry, CommandHistory};
use crate::fc::protocol::{ShaiProtocol, ShaiRequest, ShaiResponse, ResponseData};

/// Socket server for serving command history data
pub struct ShaiSessionServer {
    history: Arc<Mutex<CommandHistory>>,
    history_capacity: usize,
    socket_path: String,
    shutdown: Arc<AtomicBool>,
    pending_command: Arc<Mutex<Option<String>>>,
}

impl ShaiSessionServer {
    pub fn new(session_id: &str, history_size: usize, output_buffer_size: usize) -> Self {
        Self {
            history: Arc::new(Mutex::new(CommandHistory::new(history_size))),
            history_capacity: history_size,
            socket_path: format!("/tmp/shai_history_{}", session_id),
            shutdown: Arc::new(AtomicBool::new(false)),
            pending_command: Arc::new(Mutex::new(None)),
        }
    }

    pub fn stop(&self) {
        self.shutdown.store(true, Ordering::Relaxed);
        let _ = UnixStream::connect(&self.socket_path);
    }

    pub fn start(&self) -> Result<(), Box<dyn std::error::Error>> {
        if Path::new(&self.socket_path).exists() {
            std::fs::remove_file(&self.socket_path)?;
        }

        let listener = UnixListener::bind(&self.socket_path)?;
        let history = Arc::clone(&self.history);
        let history_capacity = self.history_capacity;
        let socket_path = self.socket_path.clone();
        let shutdown = Arc::clone(&self.shutdown);
        let pending_command = Arc::clone(&self.pending_command);

        thread::spawn(move || {
            for stream in listener.incoming() {
                if shutdown.load(Ordering::Relaxed) {
                    break;
                }

                match stream {
                    Ok(stream) => {
                        let history = Arc::clone(&history);
                        let pending_command = Arc::clone(&pending_command);
                        thread::spawn(move || {
                            if let Err(e) = Self::handle_client(stream, history, history_capacity, pending_command) {
                                eprintln!("Error handling client: {}", e);
                            }
                        });
                    }
                    Err(_) => break,
                }
            }
            
            let _ = std::fs::remove_file(&socket_path);
        });

        Ok(())
    }

    pub fn add_output(&self, data: &[u8]) {
        if let Ok(mut history) = self.history.lock() {
            if let Some(last_entry) = history.back_mut() {
                if last_entry.exit_code.is_none() {
                    last_entry.add_output(data);
                }
            }
        }
    }

    fn handle_client(
        mut stream: UnixStream,
        history: Arc<Mutex<CommandHistory>>,
        history_capacity: usize,
        pending_command: Arc<Mutex<Option<String>>>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let request = ShaiProtocol::read_request(&mut stream)?;
        
        let response = Self::process_request(request, &history, history_capacity, &pending_command);
        ShaiProtocol::write_response(&mut stream, &response)?;
        
        Ok(())
    }

    fn process_request(
        request: ShaiRequest,
        history_ref: &Arc<Mutex<CommandHistory>>,
        history_capacity: usize,
        pending_command_ref: &Arc<Mutex<Option<String>>>,
    ) -> ShaiResponse {
        match request {
            ShaiRequest::GetAllCmd => {
                let history = match history_ref.lock() {
                    Ok(h) => h,
                    Err(_) => return ShaiResponse::Error { message: "Lock error".to_string() },
                };
                
                let entries: Vec<CommandEntry> = history
                    .iter()
                    .cloned()
                    .collect();
                
                ShaiResponse::Ok { data: ResponseData::Commands(entries) }
            }
            
            ShaiRequest::GetLastCmd { n } => {
                let history = match history_ref.lock() {
                    Ok(h) => h,
                    Err(_) => return ShaiResponse::Error { message: "Lock error".to_string() },
                };
                
                let entries: Vec<CommandEntry> = history
                    .iter()
                    .rev()
                    .take(n)
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .cloned()
                    .collect();
                
                ShaiResponse::Ok { data: ResponseData::Commands(entries) }
            }
            
            ShaiRequest::Clear => {
                match history_ref.lock() {
                    Ok(mut history) => {
                        // Recreate the buffer with the original capacity instead of calling clear()
                        // because AllocRingBuffer::clear() sets capacity to 0
                        *history = CommandHistory::new(history_capacity);
                        ShaiResponse::Ok { data: ResponseData::Empty }
                    }
                    Err(_) => ShaiResponse::Error { message: "Lock error".to_string() },
                }
            }
            
            ShaiRequest::Status => {
                let history = match history_ref.lock() {
                    Ok(h) => h,
                    Err(_) => return ShaiResponse::Error { message: "Lock error".to_string() },
                };
                
                let all_commands: Vec<&CommandEntry> = history.iter().collect();
                let total = all_commands.len();
                let successful = all_commands.iter().filter(|e| e.is_success()).count();
                let failed = total - successful;
                
                let avg_duration = if total > 0 {
                    let total_duration: u64 = all_commands
                        .iter()
                        .filter_map(|e| e.duration_ms)
                        .sum();
                    Some(total_duration / total as u64)
                } else {
                    None
                };

                let stats = crate::fc::history::HistoryStats {
                    total_commands: total,
                    successful_commands: successful,
                    failed_commands: failed,
                    average_duration_ms: avg_duration,
                };
                ShaiResponse::Ok { data: ResponseData::Stats(stats) }
            }
            
            ShaiRequest::PreCmd { cmd } => {
                // Store the pending command and add it to history
                match pending_command_ref.lock() {
                    Ok(mut pending) => *pending = Some(cmd.clone()),
                    Err(_) => return ShaiResponse::Error { message: "Lock error".to_string() },
                }
                
                match history_ref.lock() {
                    Ok(mut history) => {
                        let entry = CommandEntry::new(cmd, 1024);
                        history.enqueue(entry);
                        ShaiResponse::Ok { data: ResponseData::Empty }
                    }
                    Err(_) => ShaiResponse::Error { message: "Lock error".to_string() },
                }
            }
            
            ShaiRequest::PostCmd { cmd, exit_code } => {
                // Verify the command matches the pending one
                let pending_matches = match pending_command_ref.lock() {
                    Ok(mut pending) => {
                        let matches = pending.as_ref() == Some(&cmd);
                        *pending = None; // Clear pending command
                        matches
                    }
                    Err(_) => return ShaiResponse::Error { message: "Lock error".to_string() },
                };
                
                if !pending_matches {
                    return ShaiResponse::Error { 
                        message: "PostCmd command doesn't match pending PreCmd".to_string() 
                    };
                }
                
                // Update the last command with exit code and duration
                match history_ref.lock() {
                    Ok(mut history) => {
                        if let Some(last_entry) = history.back_mut() {
                            last_entry.set_exit_code(exit_code);
                        }
                        ShaiResponse::Ok { data: ResponseData::Empty }
                    }
                    Err(_) => ShaiResponse::Error { message: "Lock error".to_string() },
                }
            }
        }
    }
}

impl Drop for ShaiSessionServer {
    fn drop(&mut self) {
        self.stop();
    }
}