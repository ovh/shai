use async_trait::async_trait;
use futures::TryStreamExt;
use rmcp::{
    model::{
        ClientCapabilities, ClientInfo, Implementation, InitializeRequestParams,
        ServerJsonRpcMessage,
    },
    service::{RunningService, ServiceExt},
    transport::Transport,
    RoleClient,
};
use std::future::Future;
use tokio::{io::AsyncBufReadExt, sync::mpsc, task::JoinHandle};
use tokio_util::io::StreamReader;

use super::mcp::{call_tool_params, content_to_string, McpClient, McpToolDescription};
use crate::tools::{ToolCall, ToolResult};

pub struct SseClient {
    url: String,
    service: Option<RunningService<RoleClient, InitializeRequestParams>>,
}

impl SseClient {
    pub fn new(url: String) -> Self {
        Self { url, service: None }
    }
}

#[async_trait]
impl McpClient for SseClient {
    async fn connect(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Only connect if not already connected
        if self.service.is_some() {
            return Ok(());
        }

        let transport = LegacySseTransport::start(&self.url).await?;
        let client_info = ClientInfo::new(
            ClientCapabilities::default(),
            Implementation::new("shai-mcp-sse-client", "0.1.0"),
        );
        let service = client_info.serve(transport).await?;
        self.service = Some(service);
        Ok(())
    }

    async fn disconnect(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if let Some(service) = self.service.take() {
            service.cancel().await?;
        }
        Ok(())
    }

    async fn list_tools(
        &self,
    ) -> Result<Vec<McpToolDescription>, Box<dyn std::error::Error + Send + Sync>> {
        let service = self.service.as_ref().ok_or("Not connected")?;
        let tools_result = service.list_tools(Default::default()).await?;

        let tool_descriptions = tools_result
            .tools
            .into_iter()
            .map(|tool| McpToolDescription {
                name: tool.name.to_string(),
                description: tool.description.unwrap_or_default().to_string(),
                parameters_schema: serde_json::Value::Object((*tool.input_schema).clone()),
            })
            .collect();

        Ok(tool_descriptions)
    }

    async fn execute_tool(
        &self,
        tool_call: ToolCall,
    ) -> Result<ToolResult, Box<dyn std::error::Error + Send + Sync>> {
        let service = self.service.as_ref().ok_or("Not connected")?;

        let result = service.call_tool(call_tool_params(&tool_call)).await?;

        Ok(ToolResult::success(content_to_string(result.content)))
    }
}

struct LegacySseTransport {
    http: reqwest::Client,
    message_endpoint: String,
    messages: mpsc::UnboundedReceiver<ServerJsonRpcMessage>,
    reader_task: Option<JoinHandle<()>>,
}

impl LegacySseTransport {
    async fn start(url: &str) -> std::io::Result<Self> {
        let response = reqwest::Client::new()
            .get(url)
            .header(reqwest::header::ACCEPT, "text/event-stream")
            .send()
            .await
            .map_err(std::io::Error::other)?
            .error_for_status()
            .map_err(std::io::Error::other)?;
        let (message_tx, messages) = mpsc::unbounded_channel();
        let (endpoint_tx, endpoint_rx) = tokio::sync::oneshot::channel();
        let base_url = url.to_string();
        let reader_task = tokio::spawn(async move {
            if let Err(error) = read_sse(response, base_url, message_tx, endpoint_tx).await {
                tracing::debug!(%error, "legacy MCP SSE stream ended");
            }
        });
        let message_endpoint = endpoint_rx
            .await
            .map_err(|_| std::io::Error::other("SSE stream ended before endpoint event"))?;
        Ok(Self {
            http: reqwest::Client::new(),
            message_endpoint,
            messages,
            reader_task: Some(reader_task),
        })
    }
}

impl Transport<RoleClient> for LegacySseTransport {
    type Error = std::io::Error;

    fn send(
        &mut self,
        item: rmcp::model::ClientJsonRpcMessage,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send + 'static {
        let http = self.http.clone();
        let endpoint = self.message_endpoint.clone();
        async move {
            http.post(endpoint)
                .json(&item)
                .send()
                .await
                .map_err(std::io::Error::other)?
                .error_for_status()
                .map_err(std::io::Error::other)?;
            Ok(())
        }
    }

    async fn receive(&mut self) -> Option<ServerJsonRpcMessage> {
        self.messages.recv().await
    }

    async fn close(&mut self) -> Result<(), Self::Error> {
        if let Some(task) = self.reader_task.take() {
            task.abort();
        }
        Ok(())
    }
}

async fn read_sse(
    response: reqwest::Response,
    base_url: String,
    messages: mpsc::UnboundedSender<ServerJsonRpcMessage>,
    endpoint: tokio::sync::oneshot::Sender<String>,
) -> std::io::Result<()> {
    let stream = response.bytes_stream().map_err(std::io::Error::other);
    let mut lines = StreamReader::new(stream).lines();
    let mut event_type = String::new();
    let mut data = String::new();
    let mut endpoint = Some(endpoint);
    while let Some(line) = lines.next_line().await? {
        if line.is_empty() {
            dispatch_event(&event_type, &data, &base_url, &messages, &mut endpoint)?;
            event_type.clear();
            data.clear();
        } else if let Some(value) = line.strip_prefix("event:") {
            event_type = value.trim().to_string();
        } else if let Some(value) = line.strip_prefix("data:") {
            data.push_str(value.strip_prefix(' ').unwrap_or(value));
            data.push('\n');
        }
    }
    dispatch_event(&event_type, &data, &base_url, &messages, &mut endpoint)
}

fn dispatch_event(
    event_type: &str,
    data: &str,
    base_url: &str,
    messages: &mpsc::UnboundedSender<ServerJsonRpcMessage>,
    endpoint: &mut Option<tokio::sync::oneshot::Sender<String>>,
) -> std::io::Result<()> {
    match event_type {
        "endpoint" => {
            if let Some(sender) = endpoint.take() {
                let url = reqwest::Url::parse(base_url)
                    .and_then(|base| base.join(data.trim()))
                    .map_err(std::io::Error::other)?
                    .to_string();
                sender
                    .send(url)
                    .map_err(|_| std::io::Error::other("endpoint receiver dropped"))?;
            }
        }
        "message" | "" if !data.trim().is_empty() => {
            let message = serde_json::from_str(data).map_err(std::io::Error::other)?;
            messages
                .send(message)
                .map_err(|_| std::io::Error::other("MCP message receiver dropped"))?;
        }
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn dispatches_endpoint_and_json_rpc_messages() {
        let (message_tx, mut message_rx) = mpsc::unbounded_channel();
        let (endpoint_tx, endpoint_rx) = tokio::sync::oneshot::channel();
        let mut endpoint = Some(endpoint_tx);

        dispatch_event(
            "endpoint",
            "/messages?sessionId=abc\n",
            "http://localhost:8000/sse",
            &message_tx,
            &mut endpoint,
        )
        .unwrap();
        assert_eq!(
            endpoint_rx.await.unwrap(),
            "http://localhost:8000/messages?sessionId=abc"
        );

        let payload = r#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2024-11-05","capabilities":{},"serverInfo":{"name":"test","version":"1.0"}}}"#;
        dispatch_event(
            "message",
            payload,
            "http://localhost:8000/sse",
            &message_tx,
            &mut endpoint,
        )
        .unwrap();
        assert!(matches!(
            message_rx.recv().await,
            Some(ServerJsonRpcMessage::Response(_))
        ));
    }

    #[tokio::test]
    async fn joins_absolute_message_endpoint_urls() {
        let (message_tx, _) = mpsc::unbounded_channel();
        let (endpoint_tx, endpoint_rx) = tokio::sync::oneshot::channel();
        let mut endpoint = Some(endpoint_tx);
        dispatch_event(
            "endpoint",
            "https://mcp.example.com/messages",
            "http://localhost:8000/sse",
            &message_tx,
            &mut endpoint,
        )
        .unwrap();
        assert_eq!(
            endpoint_rx.await.unwrap(),
            "https://mcp.example.com/messages"
        );
    }
}
