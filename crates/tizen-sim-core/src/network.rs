//! The Session's network proxy (ADR-0003): the Engine sends all its traffic
//! here, and the proxy enforces the App's Access Policy and reports every
//! request for the Network Log.
//!
//! - **WARP mode:** only what an `<access>` element grants goes through
//!   (TV guides/fundamentals/configuring-tv-applications.html, "Network
//!   access policy": "you cannot access network resources by default"; TD
//!   guides/concepts/web-runtime.md; W3C WARP, "Rules for Granting Access to
//!   a Network Resource"). HTTPS and WebSockets reach the proxy as CONNECT
//!   tunnels, so they are judged by host and port alone and never decrypted.
//!   That makes one tunnel rule for all three schemes (see `tunnel_verdict`),
//!   looser than WARP's per-scheme match: a simulator approximation.
//! - **CSP mode:** the network is open; the policy is applied to the page by
//!   the dev server instead.
//!
//! WARP says only that the user agent "rejects" a denied request. Failing it
//! as a network error, as here, is **unverified** on a TV.

use crate::app::AccessPolicy;
use crate::proxy::MAX_REQUEST_BODY;
use axum::body::Body;
use axum::http::{HeaderMap, Method, Request, Response, StatusCode, header};
use hyper::body::Incoming;
use hyper_util::rt::TokioIo;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use url::Url;

/// One request the App made over the network, for the Network Log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkRequest {
    /// The HTTP method, or `CONNECT` for a tunnel (HTTPS, WebSockets).
    pub method: String,
    /// The full URL, or `host:port` for a tunnel.
    pub url: String,
    /// The response status; `None` for a tunnel, whose responses are
    /// encrypted or not HTTP, and for a request that got no response.
    pub status: Option<u16>,
    /// Until the response head arrived, the tunnel opened, or the request
    /// was refused or failed.
    pub duration: Duration,
    pub verdict: Verdict,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// The Access Policy lets it through.
    Allowed,
    /// The Access Policy refuses it, as a TV would.
    Blocked {
        /// The `<access>` element that would allow it, e.g.
        /// `<access origin="https://api.example.com"/>`.
        missing_access: String,
    },
}

pub struct NetworkProxy {
    addr: SocketAddr,
    state: Arc<ProxyState>,
    shutdown: oneshot::Sender<()>,
    task: JoinHandle<()>,
}

struct ProxyState {
    /// The proxy's own address.
    addr: SocketAddr,
    policy: RwLock<AccessPolicy>,
    /// The dev server, which serves the App's own files and the Bridge: on a
    /// TV those are part of the App, not the network, so they always pass
    /// and aren't logged.
    own: SocketAddr,
    log: mpsc::UnboundedSender<NetworkRequest>,
    /// Whether requests go to the Network Log: only while the App runs,
    /// since what the Engine sends while starting up or shutting down isn't
    /// the App's doing.
    logging: AtomicBool,
    http: reqwest::Client,
}

impl NetworkProxy {
    /// Starts the proxy on a free local port. `own` is the dev server's
    /// address; every other request is judged by `policy` and, while logging
    /// is on (see `set_logging`), sent to `log`.
    pub async fn start(
        policy: AccessPolicy,
        own: SocketAddr,
        log: mpsc::UnboundedSender<NetworkRequest>,
    ) -> std::io::Result<NetworkProxy> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        let state = Arc::new(ProxyState {
            addr,
            policy: RwLock::new(policy),
            own,
            log,
            logging: AtomicBool::new(false),
            http: crate::proxy::client(),
        });
        let serving = state.clone();
        let (shutdown, mut shutdown_rx) = oneshot::channel::<()>();
        let task = tokio::spawn(async move {
            loop {
                let stream = tokio::select! {
                    accepted = listener.accept() => match accepted {
                        Ok((stream, _)) => stream,
                        Err(_) => continue,
                    },
                    _ = &mut shutdown_rx => return,
                };
                let state = serving.clone();
                tokio::spawn(async move {
                    let service = hyper::service::service_fn(move |request| {
                        let state = state.clone();
                        async move { state.handle(request).await }
                    });
                    let _ = hyper::server::conn::http1::Builder::new()
                        .preserve_header_case(true)
                        .serve_connection(TokioIo::new(stream), service)
                        .with_upgrades()
                        .await;
                });
            }
        });
        Ok(NetworkProxy {
            addr,
            state,
            shutdown,
            task,
        })
    }

    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// Enforces `policy` from now on, e.g. after `config.xml` changed.
    pub fn set_policy(&self, policy: AccessPolicy) {
        *self.state.policy.write().unwrap() = policy;
    }

    /// Starts or stops sending requests to the Network Log; off at first.
    /// Requests are judged either way.
    pub fn set_logging(&self, on: bool) {
        self.state.logging.store(on, Ordering::SeqCst);
    }

    pub async fn stop(self) {
        let _ = self.shutdown.send(());
        let _ = self.task.await;
    }
}

/// A request refused or whose server can't be reached: dropping the
/// connection makes the Engine fail it with a network error.
#[derive(Debug)]
struct NoResponse;

impl std::fmt::Display for NoResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("no response")
    }
}

impl std::error::Error for NoResponse {}

impl ProxyState {
    async fn handle(&self, request: Request<Incoming>) -> Result<Response<Body>, NoResponse> {
        // The Engine's own Google account checks, sent here by `--gaia-url`.
        if let Some(authority) = request.uri().authority()
            && authority
                .port_u16()
                .is_some_and(|port| is_addr(authority.host(), port, self.addr))
        {
            return Ok(empty_response(StatusCode::NOT_FOUND));
        }
        if request.method() == Method::CONNECT {
            return Ok(self.connect(request).await);
        }
        self.forward(request).await
    }

    /// Opens a tunnel to `host:port`, the way the Engine reaches HTTPS and
    /// WebSocket servers through a proxy.
    async fn connect(&self, request: Request<Incoming>) -> Response<Body> {
        let started = Instant::now();
        let Some(authority) = request.uri().authority().cloned() else {
            return empty_response(StatusCode::BAD_REQUEST);
        };
        let Some(port) = authority.port_u16() else {
            return empty_response(StatusCode::BAD_REQUEST);
        };
        let host = authority.host();
        let own = self.is_own(host, port);
        let verdict = tunnel_verdict(&self.policy.read().unwrap(), host, port);
        let entry = |verdict: Verdict| NetworkRequest {
            method: Method::CONNECT.to_string(),
            url: authority.to_string(),
            status: None,
            duration: started.elapsed(),
            verdict,
        };
        if !own && verdict != Verdict::Allowed {
            self.report(own, entry(verdict));
            // The Engine fails the request when the tunnel isn't opened.
            return empty_response(StatusCode::FORBIDDEN);
        }
        let upstream = match TcpStream::connect((host.trim_matches(['[', ']']), port)).await {
            Ok(upstream) => upstream,
            Err(error) => {
                tracing::debug!(%error, %authority, "could not open a tunnel");
                self.report(own, entry(Verdict::Allowed));
                return empty_response(StatusCode::BAD_GATEWAY);
            }
        };
        self.report(own, entry(Verdict::Allowed));
        tokio::spawn(async move {
            let Ok(engine) = hyper::upgrade::on(request).await else {
                return;
            };
            let mut engine = TokioIo::new(engine);
            let mut upstream = upstream;
            let _ = tokio::io::copy_bidirectional(&mut engine, &mut upstream).await;
        });
        empty_response(StatusCode::OK)
    }

    /// Sends a plain HTTP request on to its server and streams the answer
    /// back, untouched.
    async fn forward(&self, request: Request<Incoming>) -> Result<Response<Body>, NoResponse> {
        let started = Instant::now();
        let Ok(url) = Url::parse(&request.uri().to_string()) else {
            return Ok(empty_response(StatusCode::BAD_REQUEST));
        };
        let own = url
            .host_str()
            .zip(url.port_or_known_default())
            .is_some_and(|(host, port)| self.is_own(host, port));
        let method = request.method().clone();
        let mut entry = NetworkRequest {
            method: method.to_string(),
            url: url.to_string(),
            status: None,
            duration: Duration::ZERO,
            verdict: Verdict::Allowed,
        };
        if !own {
            let verdict = request_verdict(&self.policy.read().unwrap(), &url);
            if verdict != Verdict::Allowed {
                entry.duration = started.elapsed();
                entry.verdict = verdict;
                self.report(own, entry);
                return Err(NoResponse);
            }
        }

        let mut headers = request.headers().clone();
        strip_proxy_headers(&mut headers);
        let body =
            match axum::body::to_bytes(Body::new(request.into_body()), MAX_REQUEST_BODY).await {
                Ok(body) => body,
                Err(error) => {
                    tracing::debug!(%error, "could not read the request body");
                    return Ok(empty_response(StatusCode::BAD_REQUEST));
                }
            };
        let mut outgoing = self.http.request(method, url).headers(headers);
        if !body.is_empty() {
            outgoing = outgoing.body(body);
        }
        let upstream = outgoing.send().await;
        entry.duration = started.elapsed();
        let upstream = match upstream {
            Ok(upstream) => upstream,
            Err(error) => {
                tracing::debug!(%error, url = %entry.url, "request failed");
                self.report(own, entry);
                // A server that can't be reached is a network error too.
                return Err(NoResponse);
            }
        };
        entry.status = Some(upstream.status().as_u16());
        self.report(own, entry);
        let code = upstream.status();
        let mut headers = upstream.headers().clone();
        strip_proxy_headers(&mut headers);
        let mut response = Response::new(Body::from_stream(upstream.bytes_stream()));
        *response.status_mut() = code;
        *response.headers_mut() = headers;
        Ok(response)
    }

    /// Sends `request` to the Network Log, unless it went to the dev server
    /// (`own`) or logging is off.
    fn report(&self, own: bool, request: NetworkRequest) {
        if !own && self.logging.load(Ordering::SeqCst) {
            let _ = self.log.send(request);
        }
    }

    fn is_own(&self, host: &str, port: u16) -> bool {
        is_addr(host, port, self.own)
    }
}

/// Whether `host:port` names `addr`.
fn is_addr(host: &str, port: u16, addr: SocketAddr) -> bool {
    port == addr.port()
        && host
            .trim_matches(['[', ']'])
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip == addr.ip())
}

/// Whether the Access Policy lets a plain HTTP request to `url` through.
fn request_verdict(policy: &AccessPolicy, url: &Url) -> Verdict {
    match policy.warp_grants(url.as_str()) {
        Some(false) => Verdict::Blocked {
            missing_access: access_element(url.scheme(), url),
        },
        Some(true) | None => Verdict::Allowed,
    }
}

/// Whether the Access Policy lets a tunnel to `host:port` open. The Engine
/// tunnels HTTPS, secure and plain WebSockets alike, so a tunnel is allowed
/// if any of those schemes is granted for the host and port.
fn tunnel_verdict(policy: &AccessPolicy, host: &str, port: u16) -> Verdict {
    let urls: Vec<Url> = ["https", "wss", "ws"]
        .iter()
        .filter_map(|scheme| Url::parse(&format!("{scheme}://{host}:{port}/")).ok())
        .collect();
    if urls
        .iter()
        .any(|url| policy.warp_grants(url.as_str()) != Some(false))
    {
        return Verdict::Allowed;
    }
    // Plain WebSockets are the only thing tunnelled to port 80.
    let scheme = if port == 80 { "ws" } else { "https" };
    Verdict::Blocked {
        missing_access: urls
            .iter()
            .find(|url| url.scheme() == scheme)
            .map(|url| access_element(scheme, url))
            .unwrap_or_default(),
    }
}

/// The `<access>` element granting `url`'s host and port for `scheme`, the
/// port omitted when it is the scheme's default.
fn access_element(scheme: &str, url: &Url) -> String {
    let host = url.host_str().unwrap_or_default();
    let origin = match url.port() {
        Some(port) => format!("{scheme}://{host}:{port}"),
        None => format!("{scheme}://{host}"),
    };
    format!(r#"<access origin="{origin}"/>"#)
}

/// Strips what is meant for one connection or for the proxy itself.
fn strip_proxy_headers(headers: &mut HeaderMap) {
    crate::proxy::strip_hop_by_hop(headers);
    headers.remove(header::HOST);
    headers.remove("proxy-connection");
}

fn empty_response(code: StatusCode) -> Response<Body> {
    let mut response = Response::new(Body::empty());
    *response.status_mut() = code;
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::AccessRequest;

    fn warp(requests: Vec<AccessRequest>) -> AccessPolicy {
        AccessPolicy::Warp(requests)
    }

    fn origin(scheme: &str, host: &str, port: u16, subdomains: bool) -> AccessRequest {
        AccessRequest::Origin {
            scheme: scheme.into(),
            host: host.into(),
            port,
            subdomains,
        }
    }

    fn blocked(missing_access: &str) -> Verdict {
        Verdict::Blocked {
            missing_access: missing_access.into(),
        }
    }

    #[test]
    fn tunnels_follow_the_warp_rules_by_host_and_port() {
        let policy = warp(vec![origin("https", "example.org", 443, true)]);
        assert_eq!(
            tunnel_verdict(&policy, "example.org", 443),
            Verdict::Allowed
        );
        assert_eq!(
            tunnel_verdict(&policy, "api.example.org", 443),
            Verdict::Allowed
        );
        assert_eq!(
            tunnel_verdict(&policy, "example.org", 8443),
            blocked(r#"<access origin="https://example.org:8443"/>"#)
        );
        assert_eq!(
            tunnel_verdict(&policy, "badexample.org", 443),
            blocked(r#"<access origin="https://badexample.org"/>"#)
        );
        // Without subdomains, only the host itself.
        let policy = warp(vec![origin("https", "example.org", 443, false)]);
        assert_eq!(
            tunnel_verdict(&policy, "api.example.org", 443),
            blocked(r#"<access origin="https://api.example.org"/>"#)
        );
        // A secure WebSocket origin opens the tunnel too, and port 80 is a
        // plain WebSocket.
        let policy = warp(vec![origin("wss", "live.example.org", 443, false)]);
        assert_eq!(
            tunnel_verdict(&policy, "live.example.org", 443),
            Verdict::Allowed
        );
        assert_eq!(
            tunnel_verdict(&warp(vec![]), "live.example.org", 80),
            blocked(r#"<access origin="ws://live.example.org"/>"#)
        );
        // An http origin doesn't open tunnels.
        let policy = warp(vec![origin("http", "example.org", 443, false)]);
        assert!(matches!(
            tunnel_verdict(&policy, "example.org", 443),
            Verdict::Blocked { .. }
        ));
    }

    #[test]
    fn plain_requests_follow_the_warp_rules() {
        let policy = warp(vec![origin("http", "example.org", 80, false)]);
        let url = |u: &str| Url::parse(u).unwrap();
        assert_eq!(
            request_verdict(&policy, &url("http://example.org/data")),
            Verdict::Allowed
        );
        assert_eq!(
            request_verdict(&policy, &url("http://example.org:8080/data")),
            blocked(r#"<access origin="http://example.org:8080"/>"#)
        );
        assert_eq!(
            request_verdict(&warp(vec![AccessRequest::Any]), &url("http://a.test/")),
            Verdict::Allowed
        );
    }

    #[test]
    fn csp_mode_leaves_the_network_open() {
        let policy = AccessPolicy::Csp(crate::app::CspPolicy {
            policy: Some("default-src 'self'".into()),
            report_only: None,
            allow_navigation: None,
        });
        assert_eq!(
            tunnel_verdict(&policy, "example.org", 443),
            Verdict::Allowed
        );
        assert_eq!(
            request_verdict(&policy, &Url::parse("http://example.org/").unwrap()),
            Verdict::Allowed
        );
    }
}
