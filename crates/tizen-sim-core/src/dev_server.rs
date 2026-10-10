//! The Session's dev server: serves the App's files over local HTTP, or
//! proxies the developer's bundler for a Dev Server URL App, rewrites every
//! HTML document to load the Shim bootstrap first (ADR-0002), and hosts the
//! Bridge WebSocket.

use crate::app::AppFiles;
use crate::es_modules::{ES_MODULES_MILESTONE, needs_es_modules};
use crate::profile::VersionProfile;
use crate::proxy;
use crate::shim::{self, BOOTSTRAP_PATH, BRIDGE_PATH};
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, Request, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
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

/// What the dev server needs to serve one Session's App.
pub struct ServerSetup {
    /// Session data for the bootstrap (see `shim::session_data`).
    pub data: Value,
    /// Secret the page must present to open the Bridge.
    pub token: String,
    /// The Session's Version Profile, whose Engine the documents must suit.
    pub profile: &'static VersionProfile,
}

pub struct DevServer {
    addr: SocketAddr,
    state: Arc<ServerState>,
    shutdown: oneshot::Sender<()>,
    task: JoinHandle<()>,
}

struct ServerState {
    /// Replaced when a changed Package is unpacked again.
    files: RwLock<AppFiles>,
    /// This server's own origin, e.g. `http://127.0.0.1:1234`.
    origin: String,
    bridge_url: String,
    /// Replaced when the Session restarts with a re-read `config.xml`.
    bootstrap: RwLock<String>,
    token: String,
    /// Carries the page's messages, and the dev server's own Diagnostics.
    bridge: mpsc::UnboundedSender<BridgeMessage>,
    profile: &'static VersionProfile,
    /// Set once the ES modules Diagnostic has been raised.
    modules_diagnosed: AtomicBool,
    http: reqwest::Client,
}

impl DevServer {
    pub async fn start(
        files: &AppFiles,
        setup: ServerSetup,
        bridge: mpsc::UnboundedSender<BridgeMessage>,
    ) -> std::io::Result<DevServer> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        let bridge_url = format!("ws://{addr}{BRIDGE_PATH}?token={}", setup.token);
        let state = Arc::new(ServerState {
            files: RwLock::new(files.clone()),
            origin: format!("http://{addr}"),
            bootstrap: RwLock::new(shim::bootstrap_script(setup.data, &bridge_url)),
            bridge_url,
            token: setup.token,
            bridge,
            profile: setup.profile,
            modules_diagnosed: AtomicBool::new(false),
            http: proxy::client(),
        });
        let router = axum::Router::new()
            .route(BOOTSTRAP_PATH, get(serve_bootstrap))
            .route(BRIDGE_PATH, get(open_bridge))
            .fallback(serve_app)
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

    /// Serves the App's files from `files` from now on.
    pub fn set_files(&self, files: &AppFiles) {
        *self.state.files.write().unwrap() = files.clone();
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

/// HTML documents get the bootstrap injected; everything else is served or
/// passed through as-is.
async fn serve_app(State(state): State<Arc<ServerState>>, request: Request) -> Response {
    let files = state.files.read().unwrap().clone();
    match files {
        AppFiles::Folder(root) => serve_app_file(&state, &root, request).await,
        AppFiles::Bundler(bundler) => {
            proxy::forward(&state.http, &bundler, &state.origin, request, |html| {
                rewrite_document(&state, html)
            })
            .await
        }
    }
}

async fn serve_app_file(state: &ServerState, root: &Path, request: Request) -> Response {
    if let Some(file) = html_document(root, request.uri().path())
        && let Ok(html) = tokio::fs::read(&file).await
    {
        // No charset: the document's own <meta charset> decides, as on a TV.
        return (
            [
                (header::CONTENT_TYPE, "text/html"),
                (header::CACHE_CONTROL, "no-store"),
            ],
            rewrite_document(state, &html),
        )
            .into_response();
    }
    match ServeDir::new(root).oneshot(request).await {
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

/// Injects the bootstrap into an HTML document. First, once per Session,
/// raises a Diagnostic if the document only starts on Engines with ES modules
/// and the Session's Engine predates them, as a bundler's dev output does.
fn rewrite_document(state: &ServerState, html: &[u8]) -> Vec<u8> {
    if state.profile.milestone < ES_MODULES_MILESTONE
        && needs_es_modules(html)
        && !state.modules_diagnosed.swap(true, Ordering::SeqCst)
    {
        let message = format!(
            "The App loads its scripts as ES modules (<script type=\"module\">), which the \
             Engine of Tizen {} TVs (Chromium M{}) ignores, so they never run and the App \
             doesn't start. Serve a legacy build for this Version Profile instead, e.g. with \
             @vitejs/plugin-legacy or a non-module bundle transpiled to ES5.",
            state.profile.version, state.profile.milestone
        );
        let _ = state.bridge.send(BridgeMessage {
            method: "diagnostic".into(),
            params: json!({ "code": "es-modules-unsupported", "message": message }),
        });
    }
    shim::inject_bootstrap(html)
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

#[cfg(test)]
mod tests;
