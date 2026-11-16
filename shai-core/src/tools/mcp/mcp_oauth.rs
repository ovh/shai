use anyhow::{bail, Context};
use rmcp::transport::auth::{
    AuthorizationManager, AuthorizationMetadataSource, AuthorizationRequest, AuthorizationSession,
};
use serde::Deserialize;
use std::sync::{Arc, Mutex};
use tokio::{net::TcpListener, sync::oneshot, time::Duration};
use warp::Filter;

pub(crate) type BrowserOpener = Arc<dyn Fn(&str) + Send + Sync>;

#[derive(Debug, Deserialize)]
struct CallbackParams {
    code: Option<String>,
    state: Option<String>,
    iss: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
}

pub(crate) fn default_browser_opener() -> BrowserOpener {
    Arc::new(|url| {
        if webbrowser::open(url).is_err() {
            println!("Please open this URL in your browser: {url}");
        }
    })
}

pub(crate) async fn authorize(
    mut manager: AuthorizationManager,
    challenge: Option<String>,
    open_browser: BrowserOpener,
) -> anyhow::Result<AuthorizationManager> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .context("failed to bind OAuth callback listener")?;
    let callback_address = listener.local_addr()?;
    drop(listener);
    let redirect_uri = format!("http://{callback_address}/callback");

    let resolution = manager
        .resolve_metadata_from_challenge(challenge.as_deref())
        .await
        .context("OAuth metadata discovery failed")?;
    if resolution.source == AuthorizationMetadataSource::LegacyEndpointFallback {
        bail!("the MCP server did not publish OAuth metadata");
    }
    manager.set_metadata(resolution.metadata);

    let mut request = AuthorizationRequest::new(redirect_uri).with_client_name("Shai MCP Client");
    if let Some(challenge) = challenge {
        request = request.with_challenge(challenge);
    }
    let session = AuthorizationSession::new(manager, request)
        .await
        .map_err(|(_, error)| error)
        .context("failed to initialize OAuth authorization")?;

    let (callback_sender, callback_receiver) = oneshot::channel::<CallbackParams>();
    let callback_sender = Arc::new(Mutex::new(Some(callback_sender)));
    let callback_filter = warp::any().map({
        let callback_sender = callback_sender.clone();
        move || callback_sender.clone()
    });
    let routes = warp::get()
        .and(warp::path("callback"))
        .and(warp::query::<CallbackParams>())
        .and(callback_filter)
        .map(
            |params: CallbackParams,
             sender: Arc<Mutex<Option<oneshot::Sender<CallbackParams>>>>| {
                if let Some(sender) = sender.lock().unwrap().take() {
                    let _ = sender.send(params);
                }
                warp::reply::html("Authorization received. You can close this tab.".to_string())
            },
        );
    let (shutdown_sender, shutdown_receiver) = oneshot::channel::<()>();
    let (_, server) =
        warp::serve(routes).bind_with_graceful_shutdown(callback_address, async move {
            let _ = shutdown_receiver.await;
        });
    let server_handle = tokio::spawn(server);

    open_browser(session.get_authorization_url());
    let callback = match tokio::time::timeout(Duration::from_secs(300), callback_receiver).await {
        Ok(Ok(callback)) => callback,
        Ok(Err(_)) => {
            server_handle.abort();
            bail!("OAuth callback channel closed");
        }
        Err(_) => {
            server_handle.abort();
            bail!("timed out waiting for OAuth callback");
        }
    };
    let _ = shutdown_sender.send(());
    if let Some(error) = callback.error {
        bail!(
            "authorization failed: {error}{}",
            callback
                .error_description
                .map(|description| format!(": {description}"))
                .unwrap_or_default()
        );
    }
    let code = callback
        .code
        .context("OAuth callback did not include an authorization code")?;
    let state = callback
        .state
        .context("OAuth callback did not include a state parameter")?;
    session
        .handle_callback_with_issuer(&code, &state, callback.iss.as_deref())
        .await
        .context("OAuth token exchange failed")?;
    Ok(session.auth_manager)
}
