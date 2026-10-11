//! Dev Server URL Apps, at the Session seam: the `dev-server` fixture is
//! served by a stand-in bundler with an HMR WebSocket, and its `config.xml`
//! is copied to a throwaway directory the tests edit.

mod support;

use axum::extract::State;
use axum::extract::ws::{Message, WebSocketUpgrade};
use axum::http::header;
use axum::response::Response;
use axum::routing::get;
use serde_json::json;
use std::path::PathBuf;
use support::{STARTUP_SECS, eventually_equals, expect_event, fixture, start_source};
use tizen_sim_core::app::AppSource;
use tizen_sim_core::session::{HotReload, Lifecycle, SessionEvent};
use tokio::sync::{broadcast, mpsc};

/// A bundler-like dev server for the `dev-server` fixture. Tests push HMR
/// updates with `updates` and see what the page answers on `applied`.
struct Bundler {
    url: String,
    updates: broadcast::Sender<u32>,
    applied: mpsc::UnboundedReceiver<String>,
}

#[derive(Clone)]
struct BundlerState {
    updates: broadcast::Sender<u32>,
    applied: mpsc::UnboundedSender<String>,
}

async fn start_bundler() -> Bundler {
    let file = |name: &'static str, content_type: &'static str| {
        get(move || async move {
            let body = std::fs::read(fixture("dev-server").join(name)).unwrap();
            ([(header::CONTENT_TYPE, content_type)], body)
        })
    };
    let (updates, _) = broadcast::channel(16);
    let (applied_tx, applied) = mpsc::unbounded_channel();
    let router = axum::Router::new()
        .route("/index.html", file("index.html", "text/html"))
        .route("/app.js", file("app.js", "text/javascript"))
        .route("/hmr", get(hmr))
        .with_state(BundlerState {
            updates: updates.clone(),
            applied: applied_tx,
        });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    Bundler {
        url,
        updates,
        applied,
    }
}

/// Pushes each update to the page and reports what the page sends back.
async fn hmr(upgrade: WebSocketUpgrade, State(state): State<BundlerState>) -> Response {
    upgrade
        .protocols(["fixture-hmr"])
        .on_upgrade(move |mut socket| async move {
            let mut updates = state.updates.subscribe();
            loop {
                tokio::select! {
                    Ok(version) = updates.recv() => {
                        let update = json!({ "version": version }).to_string();
                        if socket.send(Message::Text(update.into())).await.is_err() {
                            return;
                        }
                    }
                    message = socket.recv() => match message {
                        Some(Ok(Message::Text(text))) => {
                            let _ = state.applied.send(text.to_string());
                        }
                        Some(Ok(_)) => {}
                        _ => return,
                    },
                }
            }
        })
}

/// The fixture's `config.xml`, copied so a test can edit it.
fn copy_config() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config.xml");
    std::fs::copy(fixture("dev-server").join("config.xml"), &config).unwrap();
    (dir, config)
}

fn source(bundler: &Bundler, config: PathBuf) -> AppSource {
    AppSource::DevServerUrl {
        url: bundler.url.parse().unwrap(),
        config,
    }
}

#[tokio::test]
async fn runs_an_app_from_a_bundler_with_the_shims_first() {
    let bundler = start_bundler().await;
    let (_dir, config) = copy_config();
    let mut t = start_source(source(&bundler, config), |_| {});

    expect_event(&mut t.events, STARTUP_SECS, |e| {
        matches!(e, SessionEvent::Lifecycle(Lifecycle::AppLoaded { url })
            if url.ends_with("/index.html") && !url.starts_with(&bundler.url))
    })
    .await;
    t.session.wait_until_loaded().await.unwrap();
    assert_eq!(
        t.session.evaluate("document.title").await.unwrap(),
        "Dev server fixture"
    );
    assert_eq!(
        t.session
            .evaluate("window.firstScriptSawTizen")
            .await
            .unwrap(),
        "object"
    );
    assert_eq!(
        t.session
            .evaluate("tizen.application.getCurrentApplication().appInfo.id")
            .await
            .unwrap(),
        "Fixture010.DevServer"
    );
    // The module script ran, or its `nomodule` fallback on an Engine without
    // ES modules: either way, no Diagnostic.
    eventually_equals(&t.session, "window.hmrVersion", json!(1)).await;
    t.session.stop().await;
    // Everything the Session reported, up to its exit.
    while let Some(event) = t.events.next().await {
        assert!(
            !matches!(&event, SessionEvent::Diagnostic(d) if d.code == "es-modules-unsupported"),
            "{event:?}"
        );
        if matches!(event, SessionEvent::Lifecycle(Lifecycle::Exited { .. })) {
            break;
        }
    }
}

#[tokio::test]
async fn hmr_updates_pass_through_and_apply_without_a_reload() {
    let mut bundler = start_bundler().await;
    let (_dir, config) = copy_config();
    let t = start_source(source(&bundler, config), |_| {});
    t.session.wait_until_loaded().await.unwrap();
    eventually_equals(&t.session, "window.hmrConnected === true", json!(true)).await;
    // Page state that a reload would lose.
    t.session
        .evaluate("window.pageState = 'kept'; 0")
        .await
        .unwrap();

    bundler.updates.send(2).unwrap();

    eventually_equals(&t.session, "window.hmrVersion", json!(2)).await;
    let applied = tokio::time::timeout(std::time::Duration::from_secs(10), bundler.applied.recv())
        .await
        .unwrap();
    assert_eq!(applied.as_deref(), Some("applied 2"));
    assert_eq!(
        t.session.evaluate("window.pageState").await.unwrap(),
        "kept"
    );
    t.session.stop().await;
}

/// `application.info` guards ApplicationInformation.size
/// (application.html#ApplicationInformation::size), so declaring it shows
/// the Session re-read the `config.xml`.
#[tokio::test]
async fn changing_the_config_xml_restarts_the_session() {
    let bundler = start_bundler().await;
    let (_dir, config) = copy_config();
    let mut t = start_source(source(&bundler, config.clone()), |_| {});
    t.session.wait_until_loaded().await.unwrap();
    let read_size = "(function () { try { return typeof tizen.application.getAppInfo().size; } \
                     catch (e) { return e.name; } })()";
    assert_eq!(
        t.session.evaluate(read_size).await.unwrap(),
        "SecurityError"
    );

    let privileged = std::fs::read_to_string(&config).unwrap().replace(
        "</widget>",
        r#"    <tizen:privilege name="http://tizen.org/privilege/application.info"/>
</widget>"#,
    );
    std::fs::write(&config, privileged).unwrap();

    expect_event(&mut t.events, 30, |e| {
        matches!(e, SessionEvent::HotReload(HotReload::FilesChanged { paths })
            if paths == &["config.xml"])
    })
    .await;
    expect_event(&mut t.events, 30, |e| {
        matches!(e, SessionEvent::HotReload(HotReload::Restarted { app_id })
            if app_id == "Fixture010.DevServer")
    })
    .await;
    eventually_equals(&t.session, read_size, json!("number")).await;
    eventually_equals(&t.session, "window.hmrVersion", json!(1)).await;
    t.session.stop().await;
}

#[tokio::test]
async fn a_dev_server_that_is_not_running_is_named() {
    // Bound and released, so nothing listens there.
    let free = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap();
    let (_dir, config) = copy_config();
    let url = format!("http://{free}/");
    let t = start_source(
        AppSource::DevServerUrl {
            url: url.parse().unwrap(),
            config,
        },
        |_| {},
    );
    let error = t.session.wait_until_loaded().await.unwrap_err().to_string();
    assert!(
        error.contains(&url) && error.contains("is it running"),
        "{error}"
    );
}
