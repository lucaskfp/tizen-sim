//! The Session's dev server: serves the App's files over local HTTP, rewrites
//! every HTML document to load the Shim bootstrap first (ADR-0002), and hosts
//! the Bridge WebSocket.

use crate::shim::{self, BOOTSTRAP_PATH, BRIDGE_PATH};
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, Request, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use serde_json::Value;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, RwLock};
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use tower::ServiceExt;
use tower_http::services::ServeDir;

/// A JSON-RPC notification the page sent over the Bridge.
#[derive(Debug, Clone)]
pub struct BridgeMessage {
    pub method: String,
    pub params: Value,
}

/// What the dev server needs to serve the Shims for one Session.
pub struct ShimSetup {
    /// Session data for the bootstrap (see `shim::session_data`).
    pub data: Value,
    /// Secret the page must present to open the Bridge.
    pub token: String,
}

pub struct DevServer {
    addr: SocketAddr,
    state: Arc<ServerState>,
    shutdown: oneshot::Sender<()>,
    task: JoinHandle<()>,
}

struct ServerState {
    /// Replaced when a changed Package is unpacked again.
    root: RwLock<PathBuf>,
    bridge_url: String,
    /// Replaced when the Session restarts with a re-read `config.xml`.
    bootstrap: RwLock<String>,
    token: String,
    bridge: mpsc::UnboundedSender<BridgeMessage>,
}

impl DevServer {
    pub async fn start(
        root: &Path,
        shims: ShimSetup,
        bridge: mpsc::UnboundedSender<BridgeMessage>,
    ) -> std::io::Result<DevServer> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        let bridge_url = format!("ws://{addr}{BRIDGE_PATH}?token={}", shims.token);
        let state = Arc::new(ServerState {
            root: RwLock::new(root.to_path_buf()),
            bootstrap: RwLock::new(shim::bootstrap_script(shims.data, &bridge_url)),
            bridge_url,
            token: shims.token,
            bridge,
        });
        let router = axum::Router::new()
            .route(BOOTSTRAP_PATH, get(serve_bootstrap))
            .route(BRIDGE_PATH, get(open_bridge))
            .fallback(serve_app_file)
            .with_state(state.clone());
        let (shutdown, shutdown_rx) = oneshot::channel::<()>();
        let task = tokio::spawn(async move {
            let _ = axum::serve(listener, router)
                .with_graceful_shutdown(async {
                    let _ = shutdown_rx.await;
                })
                .await;
        });
        Ok(DevServer {
            addr,
            state,
            shutdown,
            task,
        })
    }

    /// The URL of a path inside the App, e.g. its entry point.
    pub fn url(&self, path: &str) -> String {
        format!("http://{}/{}", self.addr, path.trim_start_matches('/'))
    }

    /// Serves new Session data to the pages loaded from now on.
    pub fn set_shim_data(&self, data: Value) {
        *self.state.bootstrap.write().unwrap() =
            shim::bootstrap_script(data, &self.state.bridge_url);
    }

    /// Serves the App's files from `root` from now on.
    pub fn set_root(&self, root: &Path) {
        *self.state.root.write().unwrap() = root.to_path_buf();
    }

    pub async fn stop(self) {
        let _ = self.shutdown.send(());
        let _ = self.task.await;
    }
}

async fn serve_bootstrap(State(state): State<Arc<ServerState>>) -> Response {
    (
        [
            (header::CONTENT_TYPE, "text/javascript; charset=utf-8"),
            (header::CACHE_CONTROL, "no-store"),
        ],
        state.bootstrap.read().unwrap().clone(),
    )
        .into_response()
}

/// HTML documents get the bootstrap injected; everything else is served as-is.
async fn serve_app_file(State(state): State<Arc<ServerState>>, request: Request) -> Response {
    let root = state.root.read().unwrap().clone();
    if let Some(file) = html_document(&root, request.uri().path())
        && let Ok(html) = tokio::fs::read(&file).await
    {
        // No charset: the document's own <meta charset> decides, as on a TV.
        return (
            [
                (header::CONTENT_TYPE, "text/html"),
                (header::CACHE_CONTROL, "no-store"),
            ],
            shim::inject_bootstrap(&html),
        )
            .into_response();
    }
    match ServeDir::new(&root).oneshot(request).await {
        // Revalidate every time, so a hot reload never runs a stale file.
        Ok(mut response) => {
            response.headers_mut().insert(
                header::CACHE_CONTROL,
                header::HeaderValue::from_static("no-cache"),
            );
            response.into_response()
        }
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
    }
}

/// The file for a request path if it names an HTML document inside `root`.
fn html_document(root: &Path, uri_path: &str) -> Option<PathBuf> {
    let decoded = percent_encoding::percent_decode_str(uri_path)
        .decode_utf8()
        .ok()?;
    let mut relative = PathBuf::new();
    for component in Path::new(decoded.trim_start_matches('/')).components() {
        match component {
            Component::Normal(part) => relative.push(part),
            Component::CurDir => {}
            // Never serve anything outside the App's root.
            _ => return None,
        }
    }
    if decoded.ends_with('/') || relative.as_os_str().is_empty() {
        relative.push("index.html");
    }
    let is_html = relative
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("html") || e.eq_ignore_ascii_case("htm"));
    let file = root.join(relative);
    (is_html && file.is_file()).then_some(file)
}

async fn open_bridge(
    State(state): State<Arc<ServerState>>,
    Query(query): Query<HashMap<String, String>>,
    upgrade: WebSocketUpgrade,
) -> Response {
    if query.get("token") != Some(&state.token) {
        return StatusCode::FORBIDDEN.into_response();
    }
    upgrade.on_upgrade(move |socket| run_bridge(socket, state))
}

/// The page only sends notifications so far; replies come with the first API
/// that needs a result from the Host.
async fn run_bridge(mut socket: WebSocket, state: Arc<ServerState>) {
    while let Some(Ok(message)) = socket.recv().await {
        let Message::Text(text) = message else {
            continue;
        };
        let Ok(request) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        let Some(method) = request["method"].as_str() else {
            continue;
        };
        let _ = state.bridge.send(BridgeMessage {
            method: method.to_string(),
            params: request["params"].clone(),
        });
    }
}
