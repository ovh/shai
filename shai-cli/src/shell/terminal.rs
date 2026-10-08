extern crate libc;

pub struct TerminalManager {
    original_termios: libc::termios,
}

impl TerminalManager {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let original_termios = Self::setup_raw_mode()?;
        Ok(Self { original_termios })
    }

    fn setup_raw_mode() -> Result<libc::termios, Box<dyn std::error::Error>> {
        let mut termios: libc::termios = unsafe { std::mem::zeroed() };

        if unsafe { libc::tcgetattr(libc::STDIN_FILENO, &mut termios) } == -1 {
            return Err("Failed to get terminal attributes".into());
        }

        let original = termios;

        // Set raw mode
        unsafe { libc::cfmakeraw(&mut termios) };

        if unsafe { libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &termios) } == -1 {
            return Err("Failed to set terminal attributes".into());
        }

        Ok(original)
    }

    pub fn get_window_size() -> Result<libc::winsize, Box<dyn std::error::Error>> {
        let mut ws: libc::winsize = unsafe { std::mem::zeroed() };

        if unsafe { libc::ioctl(libc::STDOUT_FILENO, libc::TIOCGWINSZ, &mut ws) } == -1 {
            return Err("Failed to get window size".into());
        }

        Ok(ws)
    }

    pub fn set_window_size(fd: i32, ws: &libc::winsize) -> Result<(), Box<dyn std::error::Error>> {
        if unsafe { libc::ioctl(fd, libc::TIOCSWINSZ, ws) } == -1 {
            return Err("Failed to set window size".into());
        }
        Ok(())
    }

    pub fn restore(&self) {
        unsafe {
            libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &self.original_termios);
        }
    }
}

impl Drop for TerminalManager {
    fn drop(&mut self) {
        self.restore();
    }
}
