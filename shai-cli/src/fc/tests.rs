#[cfg(test)]
mod tests {
    use crate::fc::client::*;
    use crate::fc::history::*;
    use crate::fc::server::*;
    use ringbuffer::RingBuffer;
    use rmp_serde::{Deserializer, Serializer};
    use serde::{Deserialize, Serialize};
    use std::thread;
    use std::time::Duration;

    fn wait_for_server_start() {
        thread::sleep(Duration::from_millis(50));
    }

    #[test]
    fn test_messagepack_serialization() {
        use ringbuffer::{AllocRingBuffer, RingBuffer};

        let mut output = AllocRingBuffer::new(10);
        for &byte in &[72, 101, 108, 108, 111] {
            // "Hello"
            output.enqueue(byte);
        }

        let entry = CommandEntry {
            cmd: "ls -la".to_string(),
            output,
            timestamp: 1234567890,
            exit_code: Some(0),
            duration_ms: Some(150),
        };

        let mut buf = Vec::new();
        entry.serialize(&mut Serializer::new(&mut buf)).unwrap();

        let mut de = Deserializer::new(&buf[..]);
        let deserialized: CommandEntry = Deserialize::deserialize(&mut de).unwrap();

        assert_eq!(entry.cmd, deserialized.cmd);
        assert_eq!(
            entry.output.iter().copied().collect::<Vec<u8>>(),
            deserialized.output.iter().copied().collect::<Vec<u8>>()
        );
        assert_eq!(entry.timestamp, deserialized.timestamp);
    }

    #[test]
    fn test_server_creation() {
        let _ = ShaiSessionServer::new("test_session_1", 100, 1000);
        // Test passes if creation succeeds without panic
    }

    #[test]
    fn test_pre_post_command_flow() {
        let session_id = "test_session_2";
        let server = ShaiSessionServer::new(session_id, 100, 1000);
        let client = ShaiSessionClient::new(session_id);

        server.start().unwrap();
        wait_for_server_start();

        // Send PreCmd
        client.pre_command("ls -la").unwrap();

        // Send PostCmd
        client.post_command(0, "ls -la").unwrap();

        // Verify command is in history
        let commands = client.get_all_commands().unwrap();
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].cmd, "ls -la");
    }

    #[test]
    fn test_mismatched_post_command() {
        let session_id = "test_session_3";
        let server = ShaiSessionServer::new(session_id, 100, 1000);
        let client = ShaiSessionClient::new(session_id);

        server.start().unwrap();
        wait_for_server_start();

        // Send PreCmd
        client.pre_command("ls -la").unwrap();

        // Send mismatched PostCmd
        let result = client.post_command(0, "pwd");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("doesn't match"));
    }

    #[test]
    fn test_get_last_commands() {
        let session_id = "test_session_4";
        let server = ShaiSessionServer::new(session_id, 100, 1000);
        let client = ShaiSessionClient::new(session_id);

        server.start().unwrap();
        wait_for_server_start();

        // Add multiple commands
        for i in 1..=5 {
            let cmd = format!("command_{}", i);
            client.pre_command(&cmd).unwrap();
            client.post_command(0, &cmd).unwrap();
        }

        // Get last 3 commands
        let last_3 = client.get_last_commands(3).unwrap();
        assert_eq!(last_3.len(), 3);
        assert_eq!(last_3[0].cmd, "command_3");
        assert_eq!(last_3[1].cmd, "command_4");
        assert_eq!(last_3[2].cmd, "command_5");
    }

    #[test]
    fn test_binary_data_efficiency() {
        let session_id = "test_session_5";
        let server = ShaiSessionServer::new(session_id, 100, 10000);
        let client = ShaiSessionClient::new(session_id);

        server.start().unwrap();
        wait_for_server_start();

        // Add command with large binary output
        client
            .pre_command("cat /dev/urandom | head -c 1000")
            .unwrap();
        client
            .post_command(0, "cat /dev/urandom | head -c 1000")
            .unwrap();

        // Retrieve and verify
        let commands = client.get_all_commands().unwrap();
        assert_eq!(commands.len(), 1);

        // MessagePack should handle binary data efficiently
        // If we get here, serialization worked
    }

    #[test]
    fn test_status_response() {
        let session_id = "test_session_6";
        let server = ShaiSessionServer::new(session_id, 100, 1000);
        let client = ShaiSessionClient::new(session_id);

        server.start().unwrap();
        wait_for_server_start();

        // Add some commands
        client.pre_command("test_cmd").unwrap();
        client.post_command(0, "test_cmd").unwrap();

        client.pre_command("failed_cmd").unwrap();
        client.post_command(1, "failed_cmd").unwrap();

        // Get status
        let stats = client.get_status().unwrap();
        assert_eq!(stats.total_commands, 2);
        assert_eq!(stats.successful_commands, 1);
        assert_eq!(stats.failed_commands, 1);
    }

    #[test]
    fn test_clear_history() {
        let session_id = "test_session_7";
        let server = ShaiSessionServer::new(session_id, 100, 1000);
        let client = ShaiSessionClient::new(session_id);

        server.start().unwrap();
        wait_for_server_start();

        // Add command
        client.pre_command("test").unwrap();
        client.post_command(0, "test").unwrap();

        // Verify command exists
        let commands_before = client.get_all_commands().unwrap();
        assert_eq!(commands_before.len(), 1);

        // Clear history
        client.clear().unwrap();

        // Verify history is empty
        let commands_after = client.get_all_commands().unwrap();
        assert_eq!(commands_after.len(), 0);
    }

    #[test]
    fn test_session_exists() {
        let session_id = "test_session_8";
        let server = ShaiSessionServer::new(session_id, 100, 1000);
        let client = ShaiSessionClient::new(session_id);

        // Before starting server
        assert!(!client.session_exists());

        // Start server
        server.start().unwrap();
        wait_for_server_start();

        // After starting server
        assert!(client.session_exists());
    }
}
