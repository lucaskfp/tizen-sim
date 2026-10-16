//! The Session's dev server: serves the App's files over local HTTP, or
//! proxies the developer's bundler for a Dev Server URL App, rewrites every
//! HTML document to load the Shim bootstrap first (ADR-0002), and hosts the
//! Bridge WebSocket.

use crate::app::{AccessPolicy, AppFiles};
use crate::csp::{self, PageCsp, ShimSources};
use crate::es_modules::{ES_MODULES_MILESTONE, needs_es_modules};
use crate::profile::VersionProfile;
use crate::proxy;
use crate::sandbox::Sandbox;
use crate::shim::{self, BOOTSTRAP_PATH, BRIDGE_PATH, SANDBOX_PATH};
use crate::web_features::WebFeatureCatalog;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, Request, State};
use axum::http::{Method, StatusCode, header};
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
    /// The App's Access Policy, whose CSP the documents get in CSP mode.
    pub access_policy: AccessPolicy,
    /// The App's Sandbox (`Home::sandbox_dir`).
    pub sandbox_dir: PathBuf,
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
    /// The headers every document gets in CSP mode; replaced with the
    /// bootstrap.
    csp: RwLock<Option<PageCsp>>,
    token: String,
    /// Carries the page's messages, and the dev server's own Diagnostics.
    bridge: mpsc::UnboundedSender<BridgeMessage>,
    profile: &'static VersionProfile,
    sandbox_dir: PathBuf,
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
            csp: RwLock::new(page_csp(&setup.access_policy, addr)),
            bridge_url,
            token: setup.token,
            bridge,
            profile: setup.profile,
            sandbox_dir: setup.sandbox_dir.clone(),
            modules_diagnosed: AtomicBool::new(false),
            http: proxy::client(),
        });
        let router = axum::Router::new()
            .route(BOOTSTRAP_PATH, get(serve_bootstrap))
            .route(BRIDGE_PATH, get(open_bridge).post(call_host))
            .nest(
                SANDBOX_PATH,
                axum::Router::new()
                    .fallback_service(ServeDir::new(&setup.sandbox_dir))
                    .layer(axum::middleware::from_fn_with_state(
                        state.clone(),
                        require_token,
                    )),
            )
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

    /// Gives documents loaded from now on `policy`'s CSP.
    pub fn set_access_policy(&self, policy: &AccessPolicy) {
        *self.state.csp.write().unwrap() = page_csp(policy, self.addr);
    }

    pub fn addr(&self) -> SocketAddr {
        self.addr
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

/// HTML documents get the bootstrap injected and the virtual `$WEBAPIS`
/// script is answered here; everything else is served or passed through
/// as-is.
async fn serve_app(State(state): State<Arc<ServerState>>, request: Request) -> Response {
    if shim::is_webapis_script(request.uri().path()) {
        return (
            [
                (header::CONTENT_TYPE, "text/javascript; charset=utf-8"),
                (header::CACHE_CONTROL, "no-store"),
            ],
            shim::WEBAPIS_SCRIPT_BODY,
        )
            .into_response();
    }
    let asks_for_favicon = is_favicon(&request);
    let files = state.files.read().unwrap().clone();
    let response = match files {
        AppFiles::Folder(root) => serve_app_file(&state, &root, request).await,
        AppFiles::Bundler(bundler) => {
            let mut rewritten = false;
            let mut response =
                proxy::forward(&state.http, &bundler, &state.origin, request, |html| {
                    rewritten = true;
                    rewrite_document(&state, html)
                })
                .await;
            if rewritten {
                add_csp(&state, response.headers_mut());
            }
            response
        }
    };
    // Every Engine asks for a favicon for a page served over HTTP (#53); an
    // App without one would see the 404 logged as its own error. A TV loads
    // the App from `file://` and asks for none (measured on a Tizen 5.0 TV,
    // see docs/agents/tizen-behaviour.md).
    if asks_for_favicon && response.status() == StatusCode::NOT_FOUND {
        return StatusCode::NO_CONTENT.into_response();
    }
    response
}

/// Whether this is the request an Engine makes for its window's icon. An
/// App's own request for it can't be told apart (same headers, measured on
/// M47, M94 and M130).
fn is_favicon(request: &Request) -> bool {
    request.method() == Method::GET
        && request.uri().path() == "/favicon.ico"
        && request.uri().query().is_none()
}

async fn serve_app_file(state: &ServerState, root: &Path, request: Request) -> Response {
    if let Some(file) = html_document(root, request.uri().path())
        && let Ok(html) = tokio::fs::read(&file).await
    {
        // No charset: the document's own <meta charset> decides, as on a TV.
        let mut response = (
            [
                (header::CONTENT_TYPE, "text/html"),
                (header::CACHE_CONTROL, "no-store"),
            ],
            rewrite_document(state, &html),
        )
            .into_response();
        add_csp(state, response.headers_mut());
        return response;
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

/// The CSP headers for a server at `addr`, letting its Shims through.
fn page_csp(policy: &AccessPolicy, addr: SocketAddr) -> Option<PageCsp> {
    csp::page_csp(
        policy,
        &ShimSources {
            script: &format!("http://{addr}{BOOTSTRAP_PATH}"),
            bridge: &format!("ws://{addr}{BRIDGE_PATH}"),
            bridge_http: &format!("http://{addr}{BRIDGE_PATH}"),
        },
    )
}

/// Adds the App's CSP to a document's headers. A policy the bundler sent
/// stays too, so the stricter of both applies, as on a TV.
fn add_csp(state: &ServerState, headers: &mut header::HeaderMap) {
    let Some(csp) = state.csp.read().unwrap().clone() else {
        return;
    };
    for (name, policy) in [
        (header::CONTENT_SECURITY_POLICY, csp.enforced),
        (header::CONTENT_SECURITY_POLICY_REPORT_ONLY, csp.report_only),
    ] {
        if let Some(value) = policy.and_then(|p| header::HeaderValue::from_str(&p).ok()) {
            headers.append(name, value);
        }
    }
}

/// Injects the bootstrap into an HTML document. First, once per Session,
/// raises a Diagnostic if the document only starts on Engines with ES modules
/// and the Session's TVs lack them, as a bundler's dev output does: their
/// Engine predates ES modules, or the web engine specifications list Module
/// objects as missing on them (Tizen 5.0, whose M63 Engine runs modules).
fn rewrite_document(state: &ServerState, html: &[u8]) -> Vec<u8> {
    let engine_lacks = state.profile.milestone < ES_MODULES_MILESTONE;
    let page_says_lacking =
        WebFeatureCatalog::bundled().lacks(&state.profile.version, "Module objects");
    if (engine_lacks || page_says_lacking)
        && needs_es_modules(html)
        && !state.modules_diagnosed.swap(true, Ordering::SeqCst)
    {
        let message = if engine_lacks {
            format!(
                "The App loads its scripts as ES modules (<script type=\"module\">), which the \
                 Engine of Tizen {} TVs (Chromium M{}) ignores, so they never run and the App \
                 doesn't start. Serve a legacy build for this Version Profile instead, e.g. with \
                 @vitejs/plugin-legacy or a non-module bundle transpiled to ES5.",
                state.profile.version, state.profile.milestone
            )
        } else {
            format!(
                "The App loads its scripts as ES modules (<script type=\"module\">). Samsung's \
                 web engine specifications list Module objects as unsupported on Tizen {} TVs, \
                 though their engine is Chromium M{}, which runs them, as this Engine does. \
                 Whether those TVs run module scripts is unverified; to be safe, serve a legacy \
                 build for this Version Profile, e.g. with @vitejs/plugin-legacy.",
                state.profile.version, state.profile.milestone
            )
        };
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

/// Lets only the page, which has the Session's token, read the Sandbox: it
/// holds the App's private files.
async fn require_token(
    State(state): State<Arc<ServerState>>,
    Query(query): Query<HashMap<String, String>>,
    request: Request,
    next: axum::middleware::Next,
) -> Response {
    if query.get("token") != Some(&state.token) {
        return StatusCode::FORBIDDEN.into_response();
    }
    next.run(request).await
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

/// A call the page waits on (ADR-0008): a JSON-RPC 2.0 request, answered
/// with its result or with the WebAPI error to raise, named in the error's
/// `data.name`.
async fn call_host(
    State(state): State<Arc<ServerState>>,
    Query(query): Query<HashMap<String, String>>,
    body: String,
) -> Response {
    if query.get("token") != Some(&state.token) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let Ok(request) = serde_json::from_str::<Value>(&body) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let id = request["id"].clone();
    let method = request["method"].as_str().unwrap_or_default().to_string();
    let params = request["params"].clone();
    let package = match &*state.files.read().unwrap() {
        AppFiles::Folder(root) => Some(root.clone()),
        AppFiles::Bundler(_) => None,
    };
    let sandbox = Sandbox::new(state.sandbox_dir.clone(), package);
    let outcome = tokio::task::spawn_blocking(move || match method.strip_prefix("filesystem.") {
        Some(call) => sandbox.call(call, &params),
        None => Err(crate::sandbox::SandboxError {
            name: "NotSupportedError",
            message: format!("the Host has no {method}"),
        }),
    })
    .await;
    let reply = match outcome {
        Ok(Ok(result)) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
        Ok(Err(error)) => json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": { "code": -32000, "message": error.message, "data": { "name": error.name } },
        }),
        Err(panic) => json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": { "code": -32603, "message": panic.to_string(), "data": { "name": "UnknownError" } },
        }),
    };
    ([(header::CACHE_CONTROL, "no-store")], axum::Json(reply)).into_response()
}

/// The page's notifications; the calls it waits on come as `POST`s instead
/// (`call_host`).
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
