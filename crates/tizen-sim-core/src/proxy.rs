//! The reverse proxy in front of a Dev Server URL App's bundler (ADR-0002):
//! HTML documents are rewritten by the caller, everything else passes through
//! untouched, including WebSocket upgrades such as the bundler's HMR socket.

use axum::body::Body;
use axum::extract::Request;
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use axum::response::{IntoResponse, Response};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use url::Url;

/// Headers that describe one connection, not the message, so a proxy never
/// forwards them (RFC 9110, section 7.6.1).
const HOP_BY_HOP: &[header::HeaderName] = &[
    header::CONNECTION,
    header::PROXY_AUTHENTICATE,
    header::PROXY_AUTHORIZATION,
    header::TE,
    header::TRAILER,
    header::TRANSFER_ENCODING,
    header::UPGRADE,
];

/// The largest request body forwarded; dev requests are far smaller.
pub(crate) const MAX_REQUEST_BODY: usize = 256 * 1024 * 1024;
/// The largest response head read while opening a WebSocket tunnel.
const MAX_UPGRADE_HEAD: usize = 64 * 1024;

/// A client for forwarding requests: it never follows redirects, so the page
/// sees them, and never goes through a system proxy.
pub fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .build()
        .expect("a plain HTTP client always builds")
}

/// Forwards `request` to the same path on `upstream`. Successful `text/html`
/// responses are buffered and passed through `rewrite_html`; every other
/// response is streamed as it arrives. `own_origin` replaces the bundler's
/// origin in redirects, so the page stays behind the proxy.
pub async fn forward(
    http: &reqwest::Client,
    upstream: &Url,
    own_origin: &str,
    request: Request,
    rewrite_html: impl FnOnce(&[u8]) -> Vec<u8>,
) -> Response {
    if is_upgrade(request.headers()) {
        return tunnel(upstream, request).await;
    }
    let target = target_url(upstream, &request);
    let method = request.method().clone();
    let mut headers = request.headers().clone();
    strip_hop_by_hop(&mut headers);
    // The bundler's own Host, which its host checks accept. Origin and
    // Referer stay as the page sent them, so CORS behaves as on the TV.
    headers.remove(header::HOST);
    // Uncompressed, so HTML can be rewritten; it's all on this machine anyway.
    headers.remove(header::ACCEPT_ENCODING);
    let body = match axum::body::to_bytes(request.into_body(), MAX_REQUEST_BODY).await {
        Ok(body) => body,
        Err(error) => return (StatusCode::BAD_REQUEST, error.to_string()).into_response(),
    };
    let mut outgoing = http.request(method.clone(), target).headers(headers);
    if !body.is_empty() {
        outgoing = outgoing.body(body);
    }
    let upstream_response = match outgoing.send().await {
        Ok(response) => response,
        Err(error) => return unreachable(upstream, &error.to_string()),
    };

    let status = upstream_response.status();
    let mut headers = upstream_response.headers().clone();
    strip_hop_by_hop(&mut headers);
    rewrite_location(&mut headers, upstream, own_origin);
    let body = if is_html(&headers) && status.is_success() && method != Method::HEAD {
        let html = match upstream_response.bytes().await {
            Ok(html) => html,
            Err(error) => return unreachable(upstream, &error.to_string()),
        };
        headers.remove(header::CONTENT_LENGTH);
        Body::from(rewrite_html(&html))
    } else {
        Body::from_stream(upstream_response.bytes_stream())
    };
    let mut response = Response::new(body);
    *response.status_mut() = status;
    *response.headers_mut() = headers;
    response
}

/// The same path and query on the bundler. Set piecewise rather than joined,
/// so a path like `//elsewhere/x` can't leave the bundler's origin.
fn target_url(upstream: &Url, request: &Request) -> Url {
    let mut target = upstream.clone();
    target.set_path(request.uri().path());
    target.set_query(request.uri().query());
    target.set_fragment(None);
    target
}

fn is_upgrade(headers: &HeaderMap) -> bool {
    headers.contains_key(header::UPGRADE) && connection_tokens(headers).any(|t| t == "upgrade")
}

/// The options in the Connection header, lowercased.
fn connection_tokens(headers: &HeaderMap) -> impl Iterator<Item = String> + '_ {
    headers
        .get_all(header::CONNECTION)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(','))
        .map(|token| token.trim().to_ascii_lowercase())
}

fn is_html(headers: &HeaderMap) -> bool {
    headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| {
            v.split(';')
                .next()
                .is_some_and(|mime| mime.trim().eq_ignore_ascii_case("text/html"))
        })
}

pub(crate) fn strip_hop_by_hop(headers: &mut HeaderMap) {
    // Headers named by Connection are hop-by-hop too.
    let named: Vec<String> = connection_tokens(headers).collect();
    for name in named {
        headers.remove(name.as_str());
    }
    for name in HOP_BY_HOP {
        headers.remove(name);
    }
    headers.remove("keep-alive");
}

/// Points a redirect to the bundler's origin at the proxy instead.
fn rewrite_location(headers: &mut HeaderMap, upstream: &Url, own_origin: &str) {
    let Some(location) = headers
        .get(header::LOCATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| Url::parse(v).ok())
    else {
        return;
    };
    if location.origin() != upstream.origin() {
        return;
    }
    let rewritten = format!("{own_origin}{}", &location[url::Position::BeforePath..]);
    if let Ok(value) = HeaderValue::from_str(&rewritten) {
        headers.insert(header::LOCATION, value);
    }
}

fn unreachable(upstream: &Url, error: &str) -> Response {
    (
        StatusCode::BAD_GATEWAY,
        format!("tizen-sim could not reach the bundler at {upstream}: {error}"),
    )
        .into_response()
}

/// Opens the upgraded connection (a WebSocket, typically the bundler's HMR
/// socket) to the bundler and, once it agrees, splices the two connections
/// together byte for byte, so subprotocols, extensions and frames pass
/// through untouched.
async fn tunnel(upstream: &Url, mut request: Request) -> Response {
    // Resolved by the URL, so IPv6 literals like [::1] work too.
    let addrs = match upstream.socket_addrs(|| None) {
        Ok(addrs) => addrs,
        Err(error) => return unreachable(upstream, &error.to_string()),
    };
    let mut bundler = match TcpStream::connect(&addrs[..]).await {
        Ok(stream) => stream,
        Err(error) => return unreachable(upstream, &error.to_string()),
    };
    let target = target_url(upstream, &request);
    let mut head = format!(
        "{} {} HTTP/1.1\r\nhost: {}\r\n",
        request.method(),
        &target[url::Position::BeforePath..url::Position::AfterQuery],
        &upstream[url::Position::BeforeHost..url::Position::AfterPort],
    )
    .into_bytes();
    for (name, value) in request.headers() {
        if name != header::HOST {
            head.extend_from_slice(name.as_str().as_bytes());
            head.extend_from_slice(b": ");
            head.extend_from_slice(value.as_bytes());
            head.extend_from_slice(b"\r\n");
        }
    }
    head.extend_from_slice(b"\r\n");
    if let Err(error) = bundler.write_all(&head).await {
        return unreachable(upstream, &error.to_string());
    }
    let (status, headers, early_bytes) = match read_response_head(&mut bundler).await {
        Ok(head) => head,
        Err(error) => return unreachable(upstream, &error),
    };
    if status != StatusCode::SWITCHING_PROTOCOLS {
        return (
            status,
            format!("the bundler answered the connection upgrade with {status}"),
        )
            .into_response();
    }

    let page = hyper::upgrade::on(&mut request);
    tokio::spawn(async move {
        let Ok(page) = page.await else { return };
        let mut page = hyper_util::rt::TokioIo::new(page);
        if page.write_all(&early_bytes).await.is_ok() {
            let _ = tokio::io::copy_bidirectional(&mut page, &mut bundler).await;
        }
    });
    let mut response = Response::new(Body::empty());
    *response.status_mut() = status;
    *response.headers_mut() = headers;
    response
}

/// Reads an HTTP/1.1 response head, returning it and any bytes the bundler
/// already sent past it (the first frames of the upgraded connection).
async fn read_response_head(
    stream: &mut TcpStream,
) -> Result<(StatusCode, HeaderMap, Vec<u8>), String> {
    let mut buffer = Vec::new();
    let end = loop {
        if let Some(end) = buffer.windows(4).position(|w| w == b"\r\n\r\n") {
            break end;
        }
        if buffer.len() > MAX_UPGRADE_HEAD {
            return Err("the response head is too large".into());
        }
        let mut chunk = [0u8; 4096];
        match stream.read(&mut chunk).await {
            Ok(0) => return Err("the connection closed before a response".into()),
            Ok(read) => buffer.extend_from_slice(&chunk[..read]),
            Err(error) => return Err(error.to_string()),
        }
    };
    let head = String::from_utf8_lossy(&buffer[..end]).into_owned();
    let mut lines = head.split("\r\n");
    let status = lines
        .next()
        .and_then(|line| line.split(' ').nth(1))
        .and_then(|code| code.parse::<u16>().ok())
        .and_then(|code| StatusCode::from_u16(code).ok())
        .ok_or_else(|| format!("malformed response: {head}"))?;
    let mut headers = HeaderMap::new();
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        if let (Ok(name), Ok(value)) = (
            header::HeaderName::from_bytes(name.trim().as_bytes()),
            HeaderValue::from_str(value.trim()),
        ) {
            headers.append(name, value);
        }
    }
    Ok((status, headers, buffer[end + 4..].to_vec()))
}
