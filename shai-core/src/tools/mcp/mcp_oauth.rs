use super::mcp_config::OAuthToken;
use oauth2::{
    basic::BasicClient, reqwest::async_http_client, url::Url, AuthType, AuthUrl, AuthorizationCode,
    ClientId, ClientSecret, CsrfToken, PkceCodeChallenge, RedirectUrl, Scope, TokenResponse,
    TokenUrl,
};
use reqwest;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use tokio::net::TcpListener;
use warp::Filter;

#[derive(Serialize)]
struct ClientRegistrationRequest {
    redirect_uris: Vec<String>,
    client_name: Option<String>,
    grant_types: Vec<String>,
    response_types: Vec<String>,
}

#[derive(Deserialize)]
struct ClientRegistrationResponse {
    client_id: String,
    client_secret: Option<String>,
}

pub async fn signin_oauth(base_url: &str) -> anyhow::Result<OAuthToken> {
    // Extract the root domain for .well-known endpoint (OAuth standard)
    let url = Url::parse(base_url)?;
    let root_url = format!("{}://{}", url.scheme(), url.host_str().unwrap_or(""));
    let well_known_url = format!("{}/.well-known/oauth-authorization-server", root_url);

    let client = reqwest::Client::new();
    let oauth_metadata: serde_json::Value =
        client.get(&well_known_url).send().await?.json().await?;

    let auth_endpoint = oauth_metadata["authorization_endpoint"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("No authorization_endpoint in OAuth metadata"))?;

    let token_endpoint = oauth_metadata["token_endpoint"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("No token_endpoint in OAuth metadata"))?;

    let registration_endpoint = oauth_metadata["registration_endpoint"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("No registration_endpoint in OAuth metadata"))?;

    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    drop(listener);

    let callback_url = format!("http://127.0.0.1:{}/callback", port);

    let registration_request = ClientRegistrationRequest {
        redirect_uris: vec![callback_url.clone()],
        client_name: Some("Shai MCP Client".to_string()),
        grant_types: vec!["authorization_code".to_string()],
        response_types: vec!["code".to_string()],
    };

    let reg_response: ClientRegistrationResponse = client
        .post(registration_endpoint)
        .json(&registration_request)
        .send()
        .await?
        .json()
        .await?;

    let client_id = ClientId::new(reg_response.client_id);
    let client_secret = reg_response.client_secret.map(ClientSecret::new);

    let oauth_client = BasicClient::new(
        client_id,
        client_secret,
        AuthUrl::new(auth_endpoint.to_string())?,
        Some(TokenUrl::new(token_endpoint.to_string())?),
    )
    .set_redirect_uri(RedirectUrl::new(callback_url)?)
    .set_auth_type(AuthType::RequestBody);

    let (pkce_challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();
    let (auth_url, csrf_token) = oauth_client
        .authorize_url(CsrfToken::new_random)
        .set_pkce_challenge(pkce_challenge)
        .add_scope(Scope::new("all".to_string()))
        .url();

    if webbrowser::open(auth_url.as_str()).is_err() {
        println!("Please open this URL in your browser: {}", auth_url);
    }

    let code_store = Arc::new(Mutex::new(None::<AuthorizationCode>));
    let csrf_store = csrf_token.secret().to_string();

    let code_store_filter = warp::any().map({
        let code_store = Arc::clone(&code_store);
        move || Arc::clone(&code_store)
    });

    let routes = warp::get()
        .and(warp::path("callback"))
        .and(warp::query::<std::collections::HashMap<String, String>>())
        .and(code_store_filter)
        .map({
            let csrf_store = csrf_store.clone();
            move |query: std::collections::HashMap<String, String>, code_store: Arc<Mutex<_>>| {
                let response = if let (Some(code), Some(state)) = (query.get("code"), query.get("state")) {
                    if state == &csrf_store {
                        let mut lock = code_store.lock().unwrap();
                        *lock = Some(AuthorizationCode::new(code.to_string()));
                        "✅ Authorization successful! You can close this tab now.".to_string()
                    } else {
                        "❌ Invalid state parameter. Authorization failed.".to_string()
                    }
                } else if let Some(error) = query.get("error") {
                    format!("❌ Authorization failed: {}", error)
                } else {
                    "❌ Missing required parameters.".to_string()
                };

                warp::reply::html(format!(
                    r#"<!DOCTYPE html>
                    <html>
                    <head>
                        <title>SHAI</title>
                        <style>
                            * {{ margin: 0; padding: 0; box-sizing: border-box; }}
                            body {{ 
                                background: linear-gradient(135deg, #667eea 0%, #f093fb 100%);
                                font-family: 'Monaco', 'Menlo', 'Consolas', monospace;
                                height: 100vh;
                                display: flex;
                                align-items: center;
                                justify-content: center;
                                padding: 20px;
                            }}
                            .terminal {{ 
                                background: #1a1a1a;
                                border: 2px solid #444;
                                border-radius: 8px;
                                padding: 30px;
                                box-shadow: 0 8px 32px rgba(0,0,0,0.3);
                                max-width: 600px;
                            }}
                            .logo {{ 
                                color: #f9e79f;
                                font-size: 0.8rem;
                                line-height: 1;
                                white-space: pre;
                                margin-bottom: 20px;
                                margin-left: -10px;
                                font-family: 'SF Mono', 'Monaco', 'Inconsolata', 'Roboto Mono', 'Source Code Pro', 'Menlo', 'Consolas', monospace;
                            }}
                            .command {{ 
                                color: #ecf0f1;
                                font-size: 0.9rem;
                                margin-top: 10px;
                                margin-left: 10px;
                            }}
                            .prompt {{ color: #f9e79f; }}
                            .divider {{
                                color: #d946ef;
                                font-size: 0.8rem;
                                margin: 15px 0;
                                opacity: 0.7;
                            }}
                        </style>
                    </head>
                    <body>
                        <div class="terminal">
                            <div class="divider">/////////////////////////////////////////////////////</div>
                            <div class="logo">  
  ███╗      ███████╗██╗  ██╗ █████╗ ██╗
  ╚═███╗    ██╔════╝██║  ██║██╔══██╗██║
     ╚═███  ███████╗███████║███████║██║
    ███╔═╝  ╚════██║██╔══██║██╔══██║██║
  ███╔═╝    ███████║██║  ██║██║  ██║██║
  ╚══╝      ╚══════╝╚═╝  ╚═╝╚═╝  ╚═╝╚═╝
                            </div>
                            <div class="divider">/////////////////////////////////////////////////////</div>
                            <div class="command"><span class="prompt">></span> {}</div>
                        </div>
                    </body>
                    </html>"#,
                    response.replace("✅ ", "").replace("❌ ", "")
                ))
            }
        });

    let (_, server) = warp::serve(routes).bind_with_graceful_shutdown(([127, 0, 0, 1], port), {
        let code_store = Arc::clone(&code_store);
        async move {
            loop {
                if code_store.lock().unwrap().is_some() {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
        }
    });

    let server_handle = tokio::spawn(server);

    let auth_code = loop {
        if let Some(code) = code_store.lock().unwrap().clone() {
            break code;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    };

    server_handle.abort();

    let token_response = oauth_client
        .exchange_code(auth_code)
        .set_pkce_verifier(pkce_verifier)
        .request_async(async_http_client)
        .await?;

    // Calculate expiration time
    let expires_at = token_response.expires_in().map(|duration| {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        now + duration.as_secs() as i64
    });

    Ok(OAuthToken {
        access_token: token_response.access_token().secret().to_string(),
        expires_at,
    })
}
