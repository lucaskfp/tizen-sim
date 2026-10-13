//! The Session's network, at the Session seam: the `network` fixture is
//! copied, given the `<access>` elements or CSP a test needs, and calls stand-in
//! API servers on this machine through the Session's proxy.

mod support;

use axum::extract::ws::{Message, WebSocketUpgrade};
use axum::routing::get;
use serde_json::{Value, json};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use support::{copy_fixture, eval_callback, expect_event, start_folder};
use tizen_sim_core::network::{NetworkRequest, Verdict};
use tizen_sim_core::session::SessionEvent;

/// A stand-in API server that sends no CORS headers. `hits` counts the
/// requests that reached it.
struct Api {
    /// e.g. `http://127.0.0.1:1234`
    origin: String,
    /// e.g. `127.0.0.1:1234`
    authority: String,
    hits: Arc<AtomicUsize>,
}

async fn start_api() -> Api {
    let hits = Arc::new(AtomicUsize::new(0));
    let counted = hits.clone();
    let router = axum::Router::new()
        .route("/data", get(|| async { "api data" }))
        .route(
            "/ws",
            get(|upgrade: WebSocketUpgrade| async move {
                upgrade.on_upgrade(|mut socket| async move {
                    while let Some(Ok(Message::Text(text))) = socket.recv().await {
                        if socket.send(Message::Text(text)).await.is_err() {
                            return;
                        }
                    }
                })
            }),
        )
        .layer(axum::middleware::from_fn(
            move |request: axum::extract::Request, next: axum::middleware::Next| {
                counted.fetch_add(1, Ordering::SeqCst);
                next.run(request)
            },
        ));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let authority = listener.local_addr().unwrap().to_string();
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    Api {
        origin: format!("http://{authority}"),
        authority,
        hits,
    }
}

/// A copy of the `network` fixture with `extra` added to its `config.xml`.
fn network_app(extra: &str) -> tempfile::TempDir {
    let app = copy_fixture("network");
    let config = app.path().join("config.xml");
    let xml = std::fs::read_to_string(&config).unwrap();
    std::fs::write(
        &config,
        xml.replace("</widget>", &format!("{extra}\n</widget>")),
    )
    .unwrap();
    app
}

async fn xhr(session: &tizen_sim_core::session::Session, url: &str) -> Value {
    eval_callback(
        session,
        &format!("request('GET', {}, done)", Value::String(url.into())),
    )
    .await
}

fn network_event(event: &SessionEvent, url: &str) -> Option<NetworkRequest> {
    match event {
        SessionEvent::Network(request) if request.url == url => Some(request.clone()),
        _ => None,
    }
}

/// The Engine's own background requests would show up as the App's, and in
/// WARP mode raise Diagnostics the App did nothing to cause. The M63 and M69
/// Engines on Windows download the Translate Ranker model from
/// www.gstatic.com within seconds of loading the App; M120 calls the
/// Optimization Guide. The nightly suite runs this on every Engine (#61).
#[tokio::test]
async fn an_app_that_makes_no_requests_has_none_in_the_network_log() {
    let mut t = support::start("hello");
    t.session.wait_until_loaded().await.unwrap();
    let idle = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
    let mut requests = vec![];
    while let Ok(Some(event)) = tokio::time::timeout_at(idle, t.events.next()).await {
        if let SessionEvent::Network(request) = event {
            requests.push(request);
        }
    }
    assert_eq!(requests, vec![]);
    t.session.stop().await;
}

#[tokio::test]
async fn warp_mode_lets_access_origins_through_and_blocks_the_rest() {
    let allowed = start_api().await;
    let other = start_api().await;
    let app = network_app(&format!(r#"<access origin="{}"/>"#, allowed.origin));
    let mut t = start_folder(app.path(), |_| {});
    t.session.wait_until_loaded().await.unwrap();

    // Cross-origin and without CORS headers, yet it succeeds, as on a TV.
    let url = format!("{}/data", allowed.origin);
    assert_eq!(
        xhr(&t.session, &url).await,
        json!({ "loaded": true, "status": 200, "body": "api data" })
    );
    let event = expect_event(&mut t.events, 10, |e| network_event(e, &url).is_some()).await;
    let request = network_event(&event, &url).unwrap();
    assert_eq!(request.method, "GET");
    assert_eq!(request.status, Some(200));
    assert_eq!(request.verdict, Verdict::Allowed);

    // Not in an <access> element: a network error, and the server never hears of it.
    let url = format!("{}/data", other.origin);
    assert_eq!(
        xhr(&t.session, &url).await,
        json!({ "loaded": false, "status": 0 })
    );
    assert_eq!(other.hits.load(Ordering::SeqCst), 0);
    let event = expect_event(&mut t.events, 10, |e| network_event(e, &url).is_some()).await;
    let request = network_event(&event, &url).unwrap();
    assert_eq!(request.status, None);
    assert!(matches!(request.verdict, Verdict::Blocked { .. }));
    let event = expect_event(
        &mut t.events,
        10,
        |e| matches!(e, SessionEvent::Diagnostic(d) if d.code == "access-not-declared"),
    )
    .await;
    let SessionEvent::Diagnostic(diagnostic) = event else {
        unreachable!()
    };
    assert!(
        diagnostic
            .message
            .contains(&format!(r#"<access origin="{}""#, other.origin)),
        "{}",
        diagnostic.message
    );

    t.session.stop().await;
}

#[tokio::test]
async fn tunnels_are_judged_by_host_and_port() {
    let allowed = start_api().await;
    let other = start_api().await;
    let app = network_app(&format!(r#"<access origin="ws://{}"/>"#, allowed.authority));
    let mut t = start_folder(app.path(), |_| {});
    t.session.wait_until_loaded().await.unwrap();

    // The Engine tunnels WebSockets through the proxy with CONNECT.
    let echo = |authority: &str| {
        format!(
            "var ws = new WebSocket('ws://{authority}/ws'); \
             ws.onopen = function () {{ ws.send('ping'); }}; \
             ws.onmessage = function (e) {{ done(e.data); ws.close(); }}; \
             ws.onerror = function () {{ done('error'); }};"
        )
    };
    assert_eq!(
        eval_callback(&t.session, &echo(&allowed.authority)).await,
        json!("ping")
    );
    let event = expect_event(&mut t.events, 10, |e| {
        network_event(e, &allowed.authority).is_some()
    })
    .await;
    let request = network_event(&event, &allowed.authority).unwrap();
    assert_eq!(request.method, "CONNECT");
    assert_eq!(request.verdict, Verdict::Allowed);

    assert_eq!(
        eval_callback(&t.session, &echo(&other.authority)).await,
        json!("error")
    );
    assert_eq!(other.hits.load(Ordering::SeqCst), 0);
    let event = expect_event(&mut t.events, 10, |e| {
        network_event(e, &other.authority).is_some()
    })
    .await;
    assert!(matches!(
        network_event(&event, &other.authority).unwrap().verdict,
        Verdict::Blocked { .. }
    ));

    t.session.stop().await;
}

#[tokio::test]
async fn csp_mode_applies_the_policy_and_leaves_the_network_open() {
    let api = start_api().await;
    let app = network_app(
        "<tizen:content-security-policy>default-src *; script-src 'self'</tizen:content-security-policy>",
    );
    let mut t = start_folder(app.path(), |_| {});
    t.session.wait_until_loaded().await.unwrap();

    // The policy refuses the fixture's inline script, and says so in the console.
    assert_eq!(
        t.session
            .evaluate("typeof window.inlineScriptRan")
            .await
            .unwrap(),
        json!("undefined")
    );
    expect_event(
        &mut t.events,
        10,
        |e| matches!(e, SessionEvent::Console(m) if m.text.contains("Content Security Policy")),
    )
    .await;
    // The Shims, which are part of the platform on a TV, are unaffected.
    assert_eq!(
        t.session.evaluate("typeof tizen").await.unwrap(),
        json!("object")
    );

    // No <access> element needed.
    let url = format!("{}/data", api.origin);
    let result = eval_callback(
        &t.session,
        &format!(
            "var xhr = new XMLHttpRequest(); xhr.open('GET', '{url}'); \
             xhr.onload = function () {{ done(xhr.responseText); }}; \
             xhr.onerror = function () {{ done('error'); }}; xhr.send();"
        ),
    )
    .await;
    assert_eq!(result, json!("api data"));
    let event = expect_event(&mut t.events, 10, |e| network_event(e, &url).is_some()).await;
    assert_eq!(
        network_event(&event, &url).unwrap().verdict,
        Verdict::Allowed
    );

    t.session.stop().await;
}
