//! The Session Registry and control endpoint (ADR-0005): a running Session
//! found through the Registry and driven over its endpoint, as another
//! terminal or the GUI would.

mod support;

use serde_json::json;
use support::{eventually_equals, expect_event, start};
use tizen_sim_core::home::Home;
use tizen_sim_core::registry::{ClientError, RegistryEntry, SessionClient, SessionRegistry};
use tizen_sim_core::session::{KeyOutcome, Lifecycle, SessionEvent, SessionStatus};

fn registry(state: &std::path::Path) -> SessionRegistry {
    SessionRegistry::new(&Home::from_env().with_state_dir(state.to_path_buf()))
}

/// The only Session in the Registry, once it is there.
fn only_entry(state: &std::path::Path) -> RegistryEntry {
    let entries = registry(state).list();
    assert_eq!(entries.len(), 1, "{entries:#?}");
    entries.into_iter().next().unwrap()
}

#[tokio::test]
async fn a_running_session_is_in_the_registry_until_it_stops() {
    let t = start("remote-keys");
    t.session.wait_until_loaded().await.unwrap();

    let entry = only_entry(t.state.path());
    assert_eq!(entry.app_id, "Fixture006.RemoteKeys");
    assert_eq!(entry.profile, "10.0");
    assert_eq!(entry.pid, std::process::id());
    assert!(!entry.token.is_empty());
    // Only a windowed Engine has a window to embed, and only on Windows (ADR-0006).
    let windowed = cfg!(windows) && std::env::var_os("TIZEN_SIM_TEST_HEADED").is_some();
    assert_eq!(entry.window.is_some(), windowed, "{entry:?}");
    let status = entry.connect().status().await.unwrap();
    assert_eq!(status, SessionStatus::Loaded);

    t.session.stop().await;
    assert!(registry(t.state.path()).list().is_empty());
    assert!(
        entry.connect().status().await.is_err(),
        "the control endpoint outlived the Session"
    );
}

#[tokio::test]
async fn the_control_endpoint_presses_keys_reloads_and_takes_screenshots() {
    let mut t = start("remote-keys");
    t.session.wait_until_loaded().await.unwrap();
    let remote = only_entry(t.state.path()).connect();

    let outcome = remote.press_key("Back").await.unwrap();
    assert_eq!(outcome, KeyOutcome::Delivered);
    eventually_equals(&t.session, "window.keyLog", json!([10009])).await;
    let outcome = remote.press_key("ColorF0Red").await.unwrap();
    assert!(
        matches!(&outcome, KeyOutcome::Dropped { reason } if reason.contains("not registered")),
        "{outcome:?}"
    );
    let err = remote.press_key("NotAKey").await.unwrap_err();
    assert!(err.to_string().contains("unknown Remote Key"), "{err}");

    // A reload starts the page afresh.
    remote.reload().await.unwrap();
    assert_eq!(
        t.session.evaluate("window.keyLog").await.unwrap(),
        json!([])
    );
    expect_event(&mut t.events, 5, |e| {
        matches!(e, SessionEvent::Lifecycle(Lifecycle::Reloaded))
    })
    .await;

    let png = remote.screenshot().await.unwrap();
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    let header = png::Decoder::new(std::io::Cursor::new(png))
        .read_info()
        .unwrap();
    assert_eq!((header.info().width, header.info().height), (1920, 1080));

    assert_eq!(
        remote.evaluate("document.title").await.unwrap(),
        json!("Remote keys fixture")
    );
    t.session.stop().await;
}

#[tokio::test]
async fn the_event_stream_replays_and_follows_the_session_until_it_exits() {
    let t = start("remote-keys");
    t.session.wait_until_loaded().await.unwrap();
    let remote = only_entry(t.state.path()).connect();

    let mut events = remote.events().await.unwrap();
    let mut seen = vec![];
    // Earlier events come first, so a client that connects late misses nothing.
    while let Some(event) = events.next().await {
        let event = event.unwrap();
        let loaded = matches!(event, SessionEvent::Lifecycle(Lifecycle::AppLoaded { .. }));
        seen.push(event);
        if loaded {
            break;
        }
    }
    assert!(
        matches!(seen[0], SessionEvent::Lifecycle(Lifecycle::Starting { .. })),
        "{seen:#?}"
    );

    t.session.press_key("Enter").await.unwrap();
    t.session.stop().await;
    let mut rest = vec![];
    while let Some(event) = events.next().await {
        rest.push(event.unwrap());
    }
    assert!(
        rest.iter().any(|e| matches!(e, SessionEvent::RemoteKey { key, outcome: KeyOutcome::Delivered } if key == "Enter")),
        "{rest:#?}"
    );
    assert!(
        matches!(
            rest.last(),
            Some(SessionEvent::Lifecycle(Lifecycle::Exited { .. }))
        ),
        "the stream should end with Exited: {rest:#?}"
    );
}

#[tokio::test]
async fn the_control_endpoint_rejects_a_missing_or_wrong_token() {
    let t = start("remote-keys");
    t.session.wait_until_loaded().await.unwrap();
    let entry = only_entry(t.state.path());

    let wrong = SessionClient::new(&entry.endpoint, "not-the-token");
    assert!(matches!(
        wrong.press_key("Back").await,
        Err(ClientError::Unauthorized)
    ));
    assert!(matches!(
        wrong.screenshot().await,
        Err(ClientError::Unauthorized)
    ));

    let missing = reqwest::Client::new()
        .post(format!("{}/key", entry.endpoint))
        .json(&json!({ "key": "Back" }))
        .send()
        .await
        .unwrap();
    assert_eq!(missing.status(), 401);

    // Neither reached the App.
    assert_eq!(
        t.session.evaluate("window.keyLog").await.unwrap(),
        json!([])
    );
    t.session.stop().await;
}
