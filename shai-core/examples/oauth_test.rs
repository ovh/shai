use shai_core::tools::mcp::{HttpClient, McpClient};

#[tokio::main]
async fn main() {
    println!("Starting OAuth flow test...");

    let mut client = HttpClient::new("https://mcp.eu.ovhcloud.com/".to_string());
    match client.connect().await {
        Ok(()) => match client.list_tools().await {
            Ok(tools) => {
                println!(
                    "OAuth flow completed successfully with {} tools available",
                    tools.len()
                );
                if let Err(error) = client.disconnect().await {
                    eprintln!("Failed to disconnect: {error}");
                    std::process::exit(1);
                }
            }
            Err(error) => {
                eprintln!("Failed to list tools after OAuth: {error}");
                std::process::exit(1);
            }
        },
        Err(error) => {
            eprintln!("OAuth flow failed: {error}");
            std::process::exit(1);
        }
    }
}
