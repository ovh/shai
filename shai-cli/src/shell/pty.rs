use libc::c_char;
use std::io::{self, Read, Write};
use std::os::unix::io::RawFd;
use std::path::PathBuf;
use std::sync::atomic::{AtomicI32, Ordering};
use std::thread;
use tempfile::NamedTempFile;

use crate::fc::server::ShaiSessionServer;
use crate::shell::rc::{Shell, ShellType, MAGIC_COOKIE};
use crate::shell::terminal::TerminalManager;

extern crate libc;

static MASTER_FD: AtomicI32 = AtomicI32::new(-1);

extern "C" fn handle_sigwinch(_: libc::c_int) {
    let master_fd = MASTER_FD.load(Ordering::Relaxed);
    if master_fd != -1 {
        if let Ok(ws) = TerminalManager::get_window_size() {
            let _ = TerminalManager::set_window_size(master_fd, &ws);
        }
    }
}

fn generate_session_id() -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    use std::time::{SystemTime, UNIX_EPOCH};

    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let pid = std::process::id();
    let thread_id = std::thread::current().id();

    let mut hasher = DefaultHasher::new();
    timestamp.hash(&mut hasher);
    pid.hash(&mut hasher);
    thread_id.hash(&mut hasher);

    let stack_var = 42u64;
    (&stack_var as *const u64 as usize).hash(&mut hasher);

    let hash = hasher.finish();

    format!("{:016x}_{}", hash, pid)
}

pub struct ShaiPtyManager {
    master_fd: RawFd,
    slave_fd: RawFd,
    session_id: String,
    temp_rc_file: Option<PathBuf>,
}

impl ShaiPtyManager {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let (master_fd, slave_fd) = Self::create_pty_pair()?;
        Ok(Self {
            master_fd,
            slave_fd,
            session_id: generate_session_id(),
            temp_rc_file: None,
        })
    }

    #[allow(dead_code)] // used by tests
    pub fn get_session_id(&self) -> &str {
        &self.session_id
    }

    fn create_pty_pair() -> Result<(RawFd, RawFd), Box<dyn std::error::Error>> {
        let master_fd = unsafe { libc::posix_openpt(libc::O_RDWR | libc::O_NOCTTY) };
        if master_fd == -1 {
            return Err("Failed to open master pty".into());
        }

        if unsafe { libc::grantpt(master_fd) } == -1 {
            unsafe { libc::close(master_fd) };
            return Err("Failed to grant pty".into());
        }

        if unsafe { libc::unlockpt(master_fd) } == -1 {
            unsafe { libc::close(master_fd) };
            return Err("Failed to unlock pty".into());
        }

        let slave_name = unsafe {
            let ptr = libc::ptsname(master_fd);
            if ptr.is_null() {
                libc::close(master_fd);
                return Err("Failed to get slave pty name".into());
            }
            std::ffi::CStr::from_ptr(ptr).to_string_lossy().into_owned()
        };

        let slave_fd = unsafe {
            libc::open(
                slave_name.as_ptr() as *const c_char,
                libc::O_RDWR | libc::O_NOCTTY,
            )
        };
        if slave_fd == -1 {
            unsafe { libc::close(master_fd) };
            return Err("Failed to open slave pty".into());
        }

        Ok((master_fd, slave_fd))
    }

    pub fn start_session(
        &mut self,
        shell: Shell,
        quiet: bool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let terminal = TerminalManager::new()?;
        let window_size = TerminalManager::get_window_size()?;
        TerminalManager::set_window_size(self.master_fd, &window_size)?;

        MASTER_FD.store(self.master_fd, Ordering::Relaxed);

        self.setup_window_resize_handler()?;

        let pid = unsafe { libc::fork() };

        if pid == 0 {
            // CHILD: Become the shell
            self.setup_child_process(shell, quiet);
        } else if pid > 0 {
            // PARENT: Handle I/O and run buffer server
            unsafe { libc::close(self.slave_fd) };

            let io_server = ShaiSessionServer::new(&self.session_id, 100, 1000);
            io_server.start()?;

            self.inject_shai_hooks(&shell)?;

            self.handle_io_forwarding(io_server, pid)?;

            MASTER_FD.store(-1, Ordering::Relaxed);
            unsafe { libc::close(self.master_fd) };
            terminal.restore();
        } else {
            // FORK FAILED
            unsafe {
                libc::close(self.master_fd);
                libc::close(self.slave_fd);
            };
            MASTER_FD.store(-1, Ordering::Relaxed);
            terminal.restore();
            return Err("Fork failed".into());
        }

        Ok(())
    }

    fn inject_shai_hooks(&mut self, shell: &Shell) -> Result<(), Box<dyn std::error::Error>> {
        // Create temp file with RC content
        let mut temp_file = NamedTempFile::new()?;
        temp_file.write_all(shell.generate_rc_content().as_bytes())?;
        temp_file.flush()?;
        let temp_path = temp_file.path().to_string_lossy();

        // create source cmd
        let source_cmd = match shell.shell_type {
            ShellType::Bash | ShellType::Sh | ShellType::Zsh | ShellType::Fish => {
                format!("source '{}'\n", temp_path)
            }
            ShellType::Powershell => {
                format!(". '{}'\n", temp_path)
            }
        };

        // Send source command to shell via stdin
        let bytes_written = unsafe {
            libc::write(
                self.master_fd,
                source_cmd.as_ptr() as *const libc::c_void,
                source_cmd.len(),
            )
        };

        if bytes_written == -1 {
            return Err("Failed to inject shai hooks".into());
        }

        let (_file, kept_path) = temp_file.keep()?;
        self.temp_rc_file = Some(kept_path);

        Ok(())
    }

    fn setup_window_resize_handler(&self) -> Result<(), Box<dyn std::error::Error>> {
        unsafe {
            let mut sa: libc::sigaction = std::mem::zeroed();
            sa.sa_sigaction = handle_sigwinch as *const () as usize;
            libc::sigemptyset(&mut sa.sa_mask);
            sa.sa_flags = libc::SA_RESTART;

            libc::sigaction(libc::SIGWINCH, &sa, std::ptr::null_mut());
        }

        Ok(())
    }

    fn setup_child_process(&self, shell: Shell, quiet: bool) -> ! {
        unsafe {
            libc::close(self.master_fd);

            // Set SHAI_SESSION_ID environment variable
            let session_env = std::ffi::CString::new("SHAI_SESSION_ID").unwrap();
            let session_value = std::ffi::CString::new(self.session_id.as_str()).unwrap();
            libc::setenv(session_env.as_ptr(), session_value.as_ptr(), 1);

            if quiet {
                let tmux_env = std::ffi::CString::new("TMUX").unwrap();
                libc::unsetenv(tmux_env.as_ptr());

                let term_session_env = std::ffi::CString::new("TERM_SESSION_ID").unwrap();
                libc::unsetenv(term_session_env.as_ptr());
            }

            libc::setsid();

            if libc::ioctl(self.slave_fd, libc::TIOCSCTTY as _, 0) == -1 {
                libc::exit(1);
            }

            libc::dup2(self.slave_fd, libc::STDIN_FILENO);
            libc::dup2(self.slave_fd, libc::STDOUT_FILENO);
            libc::dup2(self.slave_fd, libc::STDERR_FILENO);

            if self.slave_fd > 2 {
                libc::close(self.slave_fd);
            }

            let shell_cstr = std::ffi::CString::new(shell.path).unwrap();
            let interactive_arg = std::ffi::CString::new("-i").unwrap();
            libc::execl(
                shell_cstr.as_ptr(),
                shell_cstr.as_ptr(),
                interactive_arg.as_ptr(),
                std::ptr::null::<i8>(),
            );
            libc::exit(1);
        }
    }

    fn handle_io_forwarding(
        &self,
        io_server: ShaiSessionServer,
        child_pid: i32,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let master_fd_clone = self.master_fd;

        // loop to handle user input and send it to shell stdin
        let _stdin_thread = thread::spawn(move || {
            let mut stdin = io::stdin();
            let mut buffer = [0u8; 1024];

            loop {
                match stdin.read(&mut buffer) {
                    Ok(0) => break, // EOF
                    Ok(n) => {
                        let input = &buffer[..n];

                        if unsafe {
                            libc::write(master_fd_clone, input.as_ptr() as *const libc::c_void, n)
                        } == -1
                        {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        });

        let mut stdout = io::stdout();
        let mut buffer = [0u8; 1024];

        // consume until MAGIC_COOKIE is read (this is to avoid ugly sourcing echo)
        if self.temp_rc_file.is_some() {
            let cookie = MAGIC_COOKIE.to_string().into_bytes();
            let (mut i, mut b) = (0, [0u8; 1]);
            while i < cookie.len()
                && unsafe { libc::read(self.master_fd, b.as_mut_ptr() as *mut _, 1) } > 0
            {
                i = if b[0] == cookie[i] { i + 1 } else { 0 };
            }

            // then consume empty new line
            while unsafe { libc::read(self.master_fd, b.as_mut_ptr() as *mut _, 1) } > 0 {
                if b[0] != b'\r' && b[0] == b'\n' {
                    break;
                }
            }
        }

        // loop to handle shell stdout and print it to user tty
        loop {
            let bytes_read = unsafe {
                libc::read(
                    self.master_fd,
                    buffer.as_mut_ptr() as *mut libc::c_void,
                    buffer.len(),
                )
            };

            if bytes_read <= 0 {
                break;
            }

            let output_data = &buffer[..bytes_read as usize];
            io_server.add_output(output_data);

            if stdout.write_all(output_data).is_err() {
                break;
            }
            stdout.flush().ok();
        }

        // wait for both loop to end
        let mut status = 0;
        unsafe {
            libc::waitpid(child_pid, &mut status, 0);
        };

        // stop server
        io_server.stop();
        Ok(())
    }
}

impl Drop for ShaiPtyManager {
    fn drop(&mut self) {
        if let Some(temp_path) = &self.temp_rc_file {
            let _ = std::fs::remove_file(temp_path);
        }
        if self.master_fd >= 0 {
            unsafe { libc::close(self.master_fd) };
        }
        if self.slave_fd >= 0 {
            unsafe { libc::close(self.slave_fd) };
        }
    }
}

//////////////////
// TESTS
//////////////////

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn test_pty_manager_creation() {
        let pty = ShaiPtyManager::new().unwrap();

        // Should have valid file descriptors
        assert!(pty.master_fd >= 0);
        assert!(pty.slave_fd >= 0);
        assert!(pty.master_fd != pty.slave_fd);

        // Should have a session ID
        assert!(!pty.get_session_id().is_empty());
        assert!(pty.get_session_id().contains("_")); // timestamp_pid format
    }

    #[test]
    fn test_session_id_format() {
        let pty = ShaiPtyManager::new().unwrap();
        let session_id = pty.get_session_id();

        // Should be in format: hash_pid
        let parts: Vec<&str> = session_id.split('_').collect();
        assert_eq!(parts.len(), 2);

        // First part should be a hex hash (16 characters)
        let hash_part = parts[0];
        assert_eq!(hash_part.len(), 16, "Hash should be 16 hex characters");
        assert!(
            hash_part.chars().all(|c| c.is_ascii_hexdigit()),
            "Hash should only contain hex digits"
        );

        // Second part should be a PID (number)
        assert!(
            parts[1].parse::<u32>().is_ok(),
            "Second part should be a valid PID"
        );

        // Verify it matches current process PID
        let expected_pid = std::process::id();
        let actual_pid: u32 = parts[1].parse().unwrap();
        assert_eq!(actual_pid, expected_pid, "PID should match current process");
    }

    #[test]
    fn test_unique_session_ids() {
        let pty1 = ShaiPtyManager::new().unwrap();
        thread::sleep(Duration::from_millis(1)); // Ensure different timestamp
        let pty2 = ShaiPtyManager::new().unwrap();

        assert_ne!(pty1.get_session_id(), pty2.get_session_id());
    }

    #[test]
    fn test_create_pty_pair() {
        let result = ShaiPtyManager::create_pty_pair();
        assert!(result.is_ok());

        let (master_fd, slave_fd) = result.unwrap();

        // Valid file descriptors
        assert!(master_fd >= 0);
        assert!(slave_fd >= 0);
        assert_ne!(master_fd, slave_fd);

        // Clean up
        unsafe {
            libc::close(master_fd);
            libc::close(slave_fd);
        }
    }

    #[test]
    fn test_pty_pair_communication() {
        let (master_fd, slave_fd) = ShaiPtyManager::create_pty_pair().unwrap();

        // Write to master, should be readable from slave
        let test_data = b"Hello PTY\n";
        let bytes_written = unsafe {
            libc::write(
                master_fd,
                test_data.as_ptr() as *const libc::c_void,
                test_data.len(),
            )
        };
        assert_eq!(bytes_written, test_data.len() as isize);

        // Read from slave
        let mut buffer = [0u8; 64];
        let bytes_read = unsafe {
            libc::read(
                slave_fd,
                buffer.as_mut_ptr() as *mut libc::c_void,
                buffer.len(),
            )
        };

        assert!(bytes_read > 0);
        assert_eq!(&buffer[..bytes_read as usize], test_data);

        // Clean up
        unsafe {
            libc::close(master_fd);
            libc::close(slave_fd);
        }
    }

    #[test]
    fn test_multiple_pty_creation() {
        let mut ptys = Vec::new();

        // Create multiple PTYs
        for _ in 0..5 {
            let pty = ShaiPtyManager::new().unwrap();
            ptys.push(pty);
        }

        // All should have unique session IDs
        let session_ids: Vec<String> = ptys
            .iter()
            .map(|p| p.get_session_id().to_string())
            .collect();
        let mut unique_ids = session_ids.clone();
        unique_ids.sort();
        unique_ids.dedup();

        assert_eq!(session_ids.len(), unique_ids.len());

        for pty in &ptys {
            assert!(pty.master_fd >= 0);
            assert!(pty.slave_fd >= 0);
        }
    }
}
