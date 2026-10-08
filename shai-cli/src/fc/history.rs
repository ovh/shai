use console::strip_ansi_codes;
use ringbuffer::{AllocRingBuffer, RingBuffer};
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandEntry {
    pub cmd: String,
    #[serde(with = "ringbuffer_serde")]
    pub output: AllocRingBuffer<u8>,
    pub timestamp: u64,
    pub exit_code: Option<i32>,
    pub duration_ms: Option<u64>,
}

mod ringbuffer_serde {
    use super::*;
    use serde::{Deserializer, Serializer};

    pub fn serialize<S>(buffer: &AllocRingBuffer<u8>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        buffer
            .iter()
            .copied()
            .collect::<Vec<u8>>()
            .serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<AllocRingBuffer<u8>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let data: Vec<u8> = Vec::deserialize(deserializer)?;
        let mut buffer = AllocRingBuffer::new(data.len().max(1)); // Ensure non-zero capacity
        for byte in data {
            buffer.enqueue(byte);
        }
        Ok(buffer)
    }
}

impl CommandEntry {
    pub fn new(cmd: String, output_capacity: usize) -> Self {
        Self {
            cmd,
            output: AllocRingBuffer::new(output_capacity),
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            exit_code: None,
            duration_ms: None,
        }
    }

    #[allow(dead_code)] // used by tests
    pub fn with_output(cmd: String, output_data: &[u8], output_capacity: usize) -> Self {
        let mut entry = Self::new(cmd, output_capacity);
        for &byte in output_data {
            entry.output.enqueue(byte);
        }
        entry
    }

    pub fn add_output(&mut self, data: &[u8]) {
        for &byte in data {
            self.output.enqueue(byte);
        }
    }

    pub fn set_exit_code(&mut self, code: i32) {
        self.exit_code = Some(code);
    }

    #[allow(dead_code)] // used by tests
    pub fn set_duration(&mut self, duration_ms: u64) {
        self.duration_ms = Some(duration_ms);
    }

    pub fn get_output_as_string(&self) -> String {
        let bytes: Vec<u8> = self.output.iter().copied().collect();
        let s = String::from_utf8_lossy(&bytes).to_string();
        strip_ansi_codes(&s).to_string()
    }

    #[allow(dead_code)] // used by tests
    pub fn get_output_lines(&self) -> Vec<String> {
        self.get_output_as_string()
            .lines()
            .map(|s| s.to_string())
            .collect()
    }

    pub fn is_success(&self) -> bool {
        self.exit_code.unwrap_or(0) == 0
    }
}

pub type CommandHistory = AllocRingBuffer<CommandEntry>;

pub trait CommandHistoryExt {
    fn export_as_text(&self) -> String;
}

impl CommandHistoryExt for CommandHistory {
    fn export_as_text(&self) -> String {
        let mut result = String::new();
        for (i, entry) in self.iter().enumerate() {
            result.push_str(&format!("===== Command {} =====\n", i + 1));
            result.push_str(&format!("Command: {}\n", entry.cmd));
            result.push_str(&format!("Timestamp: {}\n", entry.timestamp));
            if let Some(exit_code) = entry.exit_code {
                result.push_str(&format!("Exit Code: {}\n", exit_code));
            }
            if let Some(duration) = entry.duration_ms {
                result.push_str(&format!("Duration: {}ms\n", duration));
            }
            result.push_str("Output:\n");
            result.push_str(&entry.get_output_as_string());
            result.push_str("\n\n");
        }
        result
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryStats {
    pub total_commands: usize,
    pub successful_commands: usize,
    pub failed_commands: usize,
    pub average_duration_ms: Option<u64>,
}

impl HistoryStats {
    #[allow(dead_code)] // used by tests
    pub fn success_rate(&self) -> f64 {
        if self.total_commands == 0 {
            0.0
        } else {
            self.successful_commands as f64 / self.total_commands as f64
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_entry_creation() {
        let entry = CommandEntry::new("ls -la".to_string(), 1024);
        assert_eq!(entry.cmd, "ls -la");
        assert_eq!(entry.output.capacity(), 1024);
        assert!(entry.exit_code.is_none());
        assert!(entry.duration_ms.is_none());
    }

    #[test]
    fn test_command_entry_with_output() {
        let output = b"file1.txt\nfile2.txt\n";
        let entry = CommandEntry::with_output("ls".to_string(), output, 1024);

        assert_eq!(entry.cmd, "ls");
        assert_eq!(entry.get_output_as_string(), "file1.txt\nfile2.txt\n");

        let lines = entry.get_output_lines();
        assert_eq!(lines, vec!["file1.txt", "file2.txt"]);
    }

    #[test]
    fn test_command_entry_modification() {
        let mut entry = CommandEntry::new("echo hello".to_string(), 1024);

        entry.add_output(b"hello\n");
        entry.set_exit_code(0);
        entry.set_duration(15);

        assert_eq!(entry.get_output_as_string(), "hello\n");
        assert_eq!(entry.exit_code, Some(0));
        assert_eq!(entry.duration_ms, Some(15));
        assert!(entry.is_success());
    }

    #[test]
    fn test_command_history_basic() {
        let mut history = CommandHistory::new(10);

        assert!(history.is_empty());
        assert_eq!(history.len(), 0);
        assert_eq!(history.capacity(), 10);

        history.enqueue(CommandEntry::new("ls".to_string(), 1024));
        history.enqueue(CommandEntry::new("pwd".to_string(), 1024));

        assert_eq!(history.len(), 2);
        assert!(!history.is_empty());

        let last = history.back().unwrap();
        assert_eq!(last.cmd, "pwd");
    }

    #[test]
    fn test_command_history_with_output() {
        let mut history = CommandHistory::new(5);

        history.enqueue(CommandEntry::with_output(
            "echo hello".to_string(),
            b"hello\n",
            1024,
        ));
        history.enqueue(CommandEntry::with_output(
            "echo world".to_string(),
            b"world\n",
            1024,
        ));

        let commands: Vec<&CommandEntry> = history.iter().collect();
        assert_eq!(commands.len(), 2);
        assert_eq!(commands[0].get_output_as_string(), "hello\n");
        assert_eq!(commands[1].get_output_as_string(), "world\n");
    }

    #[test]
    fn test_command_history_wrap_around() {
        let mut history = CommandHistory::new(3);

        // Add more commands than capacity
        for i in 1..=5 {
            history.enqueue(CommandEntry::new(format!("command{}", i), 1024));
        }

        assert_eq!(history.len(), 3);
        let commands: Vec<&CommandEntry> = history.iter().collect();
        assert_eq!(commands[0].cmd, "command3");
        assert_eq!(commands[1].cmd, "command4");
        assert_eq!(commands[2].cmd, "command5");
    }

    #[test]
    fn test_find_commands() {
        let mut history = CommandHistory::new(10);

        history.enqueue(CommandEntry::new("ls -la".to_string(), 1024));
        history.enqueue(CommandEntry::new("grep pattern file.txt".to_string(), 1024));
        history.enqueue(CommandEntry::new("ls -l".to_string(), 1024));
        history.enqueue(CommandEntry::new("cat file.txt".to_string(), 1024));

        let ls_commands: Vec<&CommandEntry> = history
            .iter()
            .filter(|entry| entry.cmd.contains("ls"))
            .collect();
        assert_eq!(ls_commands.len(), 2);
        assert_eq!(ls_commands[0].cmd, "ls -la");
        assert_eq!(ls_commands[1].cmd, "ls -l");

        let file_commands: Vec<&CommandEntry> = history
            .iter()
            .filter(|entry| entry.cmd.contains("file.txt"))
            .collect();
        assert_eq!(file_commands.len(), 2);
    }

    #[test]
    fn test_command_filtering_by_exit_code() {
        let mut history = CommandHistory::new(10);

        // Add commands with different exit codes
        let mut entry1 = CommandEntry::new("successful_cmd".to_string(), 1024);
        entry1.set_exit_code(0);
        history.enqueue(entry1);

        let mut entry2 = CommandEntry::new("failed_cmd".to_string(), 1024);
        entry2.set_exit_code(1);
        history.enqueue(entry2);

        let mut entry3 = CommandEntry::new("another_success".to_string(), 1024);
        entry3.set_exit_code(0);
        history.enqueue(entry3);

        let successful: Vec<&CommandEntry> =
            history.iter().filter(|entry| entry.is_success()).collect();
        assert_eq!(successful.len(), 2);

        let failed: Vec<&CommandEntry> =
            history.iter().filter(|entry| !entry.is_success()).collect();
        assert_eq!(failed.len(), 1);
        assert_eq!(failed[0].cmd, "failed_cmd");

        let exit_code_1: Vec<&CommandEntry> = history
            .iter()
            .filter(|entry| entry.exit_code == Some(1))
            .collect();
        assert_eq!(exit_code_1.len(), 1);
        assert_eq!(exit_code_1[0].cmd, "failed_cmd");
    }

    #[test]
    fn test_history_stats() {
        let mut history = CommandHistory::new(10);

        // Add some commands with different outcomes
        let mut entry1 = CommandEntry::new("cmd1".to_string(), 1024);
        entry1.set_exit_code(0);
        entry1.set_duration(100);
        history.enqueue(entry1);

        let mut entry2 = CommandEntry::new("cmd2".to_string(), 1024);
        entry2.set_exit_code(1);
        entry2.set_duration(200);
        history.enqueue(entry2);

        let mut entry3 = CommandEntry::new("cmd3".to_string(), 1024);
        entry3.set_exit_code(0);
        entry3.set_duration(300);
        history.enqueue(entry3);

        let all_commands: Vec<&CommandEntry> = history.iter().collect();
        let total = all_commands.len();
        let successful = all_commands.iter().filter(|e| e.is_success()).count();
        let failed = total - successful;

        let avg_duration = if total > 0 {
            let total_duration: u64 = all_commands.iter().filter_map(|e| e.duration_ms).sum();
            Some(total_duration / total as u64)
        } else {
            None
        };

        let stats = HistoryStats {
            total_commands: total,
            successful_commands: successful,
            failed_commands: failed,
            average_duration_ms: avg_duration,
        };

        assert_eq!(stats.total_commands, 3);
        assert_eq!(stats.successful_commands, 2);
        assert_eq!(stats.failed_commands, 1);
        assert_eq!(stats.average_duration_ms, Some(200));
        assert_eq!(stats.success_rate(), 2.0 / 3.0);
    }

    #[test]
    fn test_get_last_commands() {
        let mut history = CommandHistory::new(10);

        for i in 1..=5 {
            history.enqueue(CommandEntry::new(format!("command{}", i), 1024));
        }

        let last_3: Vec<&CommandEntry> = history
            .iter()
            .rev()
            .take(3)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        assert_eq!(last_3.len(), 3);
        assert_eq!(last_3[0].cmd, "command3");
        assert_eq!(last_3[1].cmd, "command4");
        assert_eq!(last_3[2].cmd, "command5");

        let last_10: Vec<&CommandEntry> = history
            .iter()
            .rev()
            .take(10)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        assert_eq!(last_10.len(), 5); // Only 5 commands exist
    }

    #[test]
    fn test_clear_history() {
        let mut history = CommandHistory::new(10);

        history.enqueue(CommandEntry::new("test".to_string(), 1024));
        assert_eq!(history.len(), 1);

        history.clear();
        assert_eq!(history.len(), 0);
        assert!(history.is_empty());
    }

    #[test]
    fn test_export_as_text() {
        use super::CommandHistoryExt;
        let mut history = CommandHistory::new(10);

        history.enqueue(CommandEntry::with_output(
            "echo hello".to_string(),
            b"hello\n",
            1024,
        ));

        let exported = history.export_as_text();
        assert!(exported.contains("Command: echo hello"));
        assert!(exported.contains("Output:\nhello"));
    }
}
