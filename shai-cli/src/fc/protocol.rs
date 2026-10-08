use rmp_serde::{Deserializer, Serializer};
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;

use crate::fc::history::{CommandEntry, HistoryStats};

#[derive(Debug, Serialize, Deserialize)]
pub enum ShaiRequest {
    // send signals
    PreCmd { cmd: String },
    PostCmd { cmd: String, exit_code: i32 },

    // request data
    GetAllCmd,
    GetLastCmd { n: usize },
    Clear,
    Status,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum ShaiResponse {
    Ok { data: ResponseData },
    Error { message: String },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ResponseData {
    Commands(Vec<CommandEntry>),
    Stats(HistoryStats),
    Empty,
}

pub struct ShaiProtocol;

impl ShaiProtocol {
    pub fn write_request(
        stream: &mut UnixStream,
        request: &ShaiRequest,
    ) -> Result<(), Box<dyn std::error::Error>> {
        Self::write_message(stream, request)
    }

    pub fn read_request(
        stream: &mut UnixStream,
    ) -> Result<ShaiRequest, Box<dyn std::error::Error>> {
        Self::read_message_request(stream)
    }

    pub fn write_response(
        stream: &mut UnixStream,
        response: &ShaiResponse,
    ) -> Result<(), Box<dyn std::error::Error>> {
        Self::write_message(stream, response)
    }

    pub fn read_response(
        stream: &mut UnixStream,
    ) -> Result<ShaiResponse, Box<dyn std::error::Error>> {
        Self::read_message_response(stream)
    }

    // Generic write method - eliminates duplication for writing
    fn write_message<T: Serialize>(
        stream: &mut UnixStream,
        message: &T,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut buf = Vec::new();
        message.serialize(&mut Serializer::new(&mut buf))?;

        // Write length prefix (4 bytes) then data
        stream.write_all(&(buf.len() as u32).to_le_bytes())?;
        stream.write_all(&buf)?;
        stream.flush()?;

        Ok(())
    }

    // Specific read method for requests
    fn read_message_request(
        stream: &mut UnixStream,
    ) -> Result<ShaiRequest, Box<dyn std::error::Error>> {
        // Read length prefix
        let mut len_buf = [0u8; 4];
        stream.read_exact(&mut len_buf)?;
        let len = u32::from_le_bytes(len_buf) as usize;

        // Read data
        let mut buf = vec![0u8; len];
        stream.read_exact(&mut buf)?;

        let mut de = Deserializer::new(&buf[..]);
        let request = ShaiRequest::deserialize(&mut de)?;

        Ok(request)
    }

    // Specific read method for responses
    fn read_message_response(
        stream: &mut UnixStream,
    ) -> Result<ShaiResponse, Box<dyn std::error::Error>> {
        // Read length prefix
        let mut len_buf = [0u8; 4];
        stream.read_exact(&mut len_buf)?;
        let len = u32::from_le_bytes(len_buf) as usize;

        // Read data
        let mut buf = vec![0u8; len];
        stream.read_exact(&mut buf)?;

        let mut de = Deserializer::new(&buf[..]);
        let response = ShaiResponse::deserialize(&mut de)?;

        Ok(response)
    }
}
