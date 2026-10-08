use shai_core::tools::mcp::mcp_oauth::signin_oauth;

#[tokio::main]
async fn main() {
    println!("🚀 Starting OAuth flow test...");

    match signin_oauth("https://mcp.eu.ovhcloud.com/").await {
        Ok(token) => {
            println!("✅ OAuth flow completed successfully!");
            println!("🎫 Access Token: {}", token.access_token);
            println!("🔑 Token length: {} characters", token.access_token.len());
            if let Some(expires_at) = token.expires_at {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs() as i64;
                let seconds_until_expiry = expires_at - now;
                println!("⏰ Token expires in {} seconds", seconds_until_expiry);
            } else {
                println!("⏰ Token has no expiration");
            }
        }
        Err(e) => {
            println!("❌ OAuth flow failed: {}", e);
            std::process::exit(1);
        }
    }
}
