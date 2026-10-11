//! The proxy in front of a bundler, without an Engine: requests go through a
//! DevServer to a stand-in bundler.

use super::*;
use axum::response::Redirect;
use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite;
use url::Url;

const MODULE_PAGE: &str =
    r#"<!DOCTYPE html><html><head><script type="module" src="/main.js"></script></head></html>"#;

/// A stand-in bundler: an HTML page, a script, redirects and an HMR
/// WebSocket that reports the Host it saw, then echoes.
async fn start_bundler() -> Url {
    async fn hmr(upgrade: WebSocketUpgrade, headers: axum::http::HeaderMap) -> Response {
        let host = headers[header::HOST].to_str().unwrap().to_string();
        upgrade
            .protocols(["fixture-hmr"])
            .on_upgrade(move |mut socket| async move {
                let _ = socket
                    .send(Message::Text(format!("host {host}").into()))
                    .await;
                while let Some(Ok(Message::Text(text))) = socket.recv().await {
                    let _ = socket
                        .send(Message::Text(format!("echo {}", text.as_str()).into()))
                        .await;
                }
            })
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    // Some bundlers redirect to their own absolute URLs.
    let absolute = format!("http://{addr}/new");
    let router = axum::Router::new()
        .route("/", get(|| async { axum::response::Html(MODULE_PAGE) }))
        .route(
            "/main.js",
            get(|| async { ([(header::CONTENT_TYPE, "text/javascript")], "export {};") }),
        )
        .route("/old", get(|| async { Redirect::temporary("/new") }))
        .route(
            "/absolute",
            get(move || async move { Redirect::temporary(&absolute) }),
        )
        .route("/hmr", get(hmr));
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    format!("http://{addr}/").parse().unwrap()
}

fn profile(milestone: u32) -> &'static VersionProfile {
    let mut profile = VersionProfile::latest().clone();
    profile.version = format!("M{milestone}");
    profile.milestone = milestone;
    Box::leak(Box::new(profile))
}

async fn start_proxy(
    bundler: &Url,
    milestone: u32,
) -> (DevServer, mpsc::UnboundedReceiver<BridgeMessage>) {
    let (bridge, messages) = mpsc::unbounded_channel();
    let setup = ServerSetup {
        data: json!({}),
        token: "token".into(),
        profile: profile(milestone),
        access_policy: crate::app::AccessPolicy::Warp(vec![]),
        sandbox_dir: std::env::temp_dir().join("tizen-sim-test-sandbox"),
    };
    let server = DevServer::start(&AppFiles::Bundler(bundler.clone()), setup, bridge)
        .await
        .unwrap();
    (server, messages)
}

async fn get_text(server: &DevServer, path: &str) -> String {
    proxy::client()
        .get(server.url(path))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap()
}

#[tokio::test]
async fn html_gets_the_bootstrap_and_other_files_pass_through() {
    let (server, _) = start_proxy(&start_bundler().await, 130).await;
    let http = proxy::client();

    let page = http.get(server.url("/")).send().await.unwrap();
    let content_type = page.headers()[header::CONTENT_TYPE].to_str().unwrap();
    assert!(content_type.starts_with("text/html"), "{content_type}");
    let html = page.text().await.unwrap();
    let expected = format!(
        r#"<!DOCTYPE html><html><head><script src="{BOOTSTRAP_PATH}"></script><script type="module""#
    );
    assert!(html.starts_with(&expected), "{html}");

    let script = http.get(server.url("/main.js")).send().await.unwrap();
    assert_eq!(script.headers()[header::CONTENT_TYPE], "text/javascript");
    assert_eq!(script.text().await.unwrap(), "export {};");

    let missing = http.get(server.url("/absent")).send().await.unwrap();
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);

    // The Shim bootstrap is ours, not the bundler's.
    assert!(
        get_text(&server, BOOTSTRAP_PATH)
            .await
            .starts_with("window.__TIZEN_SIM__")
    );
    server.stop().await;
}

/// The bundler never sees `$WEBAPIS`: the dev server answers it at any depth.
#[tokio::test]
async fn the_webapis_script_is_ours_at_any_depth() {
    let (server, _) = start_proxy(&start_bundler().await, 130).await;
    for path in [
        "/$WEBAPIS/webapis/webapis.js",
        "/a/b/$WEBAPIS/webapis/webapis.js",
    ] {
        assert_eq!(get_text(&server, path).await, shim::WEBAPIS_SCRIPT_BODY);
    }
    server.stop().await;
}

#[tokio::test]
async fn redirects_stay_behind_the_proxy() {
    let (server, _) = start_proxy(&start_bundler().await, 130).await;
    let http = proxy::client();
    let relative = http.get(server.url("/old")).send().await.unwrap();
    assert_eq!(relative.headers()[header::LOCATION], "/new");
    let absolute = http.get(server.url("/absolute")).send().await.unwrap();
    assert_eq!(
        absolute.headers()[header::LOCATION],
        server.url("/new").as_str()
    );
    server.stop().await;
}

#[tokio::test]
async fn websockets_pass_through_with_their_subprotocol() {
    let bundler = start_bundler().await;
    let (server, _) = start_proxy(&bundler, 130).await;
    let url = server.url("/hmr").replace("http://", "ws://");
    let mut request = tungstenite::client::IntoClientRequest::into_client_request(url).unwrap();
    request
        .headers_mut()
        .insert("sec-websocket-protocol", "fixture-hmr".parse().unwrap());
    let (mut socket, response) = tokio_tungstenite::connect_async(request).await.unwrap();
    assert_eq!(response.headers()["sec-websocket-protocol"], "fixture-hmr");

    // The bundler sees its own Host, which its host checks accept.
    let host = &bundler[url::Position::BeforeHost..url::Position::AfterPort];
    let greeting = socket.next().await.unwrap().unwrap();
    assert_eq!(
        greeting.into_text().unwrap().as_str(),
        format!("host {host}")
    );
    socket
        .send(tungstenite::Message::Text("ping".into()))
        .await
        .unwrap();
    let echo = socket.next().await.unwrap().unwrap();
    assert_eq!(echo.into_text().unwrap().as_str(), "echo ping");
    server.stop().await;
}

#[tokio::test]
async fn module_scripts_raise_a_diagnostic_once_on_an_engine_without_es_modules() {
    let (server, mut messages) = start_proxy(&start_bundler().await, 56).await;
    for _ in 0..2 {
        let html = get_text(&server, "/").await;
        assert!(
            html.contains(BOOTSTRAP_PATH),
            "the App is still served: {html}"
        );
    }
    let message = messages.try_recv().expect("a Diagnostic");
    assert_eq!(message.method, "diagnostic");
    assert_eq!(message.params["code"], "es-modules-unsupported");
    let text = message.params["message"].as_str().unwrap();
    assert!(
        text.contains("M56") && text.contains("legacy build"),
        "{text}"
    );
    assert!(messages.try_recv().is_err(), "raised more than once");
    server.stop().await;
}

#[tokio::test]
async fn module_scripts_are_fine_on_an_engine_with_es_modules() {
    let (server, mut messages) = start_proxy(&start_bundler().await, ES_MODULES_MILESTONE).await;
    get_text(&server, "/").await;
    assert!(messages.try_recv().is_err());
    server.stop().await;
}

#[tokio::test]
async fn an_unreachable_bundler_is_a_bad_gateway() {
    // Bound and released, so nothing listens there.
    let free = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap()
        .local_addr()
        .unwrap();
    let bundler: Url = format!("http://{free}/").parse().unwrap();
    let (server, _) = start_proxy(&bundler, 130).await;
    let response = proxy::client().get(server.url("/")).send().await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    assert!(response.text().await.unwrap().contains(&free.to_string()));
    server.stop().await;
}
