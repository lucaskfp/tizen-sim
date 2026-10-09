//! Shared harness for tests at the Session seam.
#![allow(dead_code)]

use serde_json::Value;
use std::path::PathBuf;
use std::time::Duration;
use tizen_sim_core::app::AppSource;
use tizen_sim_core::home::Home;
use tizen_sim_core::session::{Events, Session, SessionConfig, SessionEvent};

pub fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/apps")
        .join(name)
}

/// A Session on a fixture App. Engines come from the shared per-user cache so
/// they are downloaded once; everything else lives in a throwaway directory.
pub struct TestSession {
    pub session: Session,
    pub events: Events,
    _state: tempfile::TempDir,
}

pub fn start(fixture_name: &str) -> TestSession {
    start_with(fixture_name, |_| {})
}

/// Like `start`, with a chance to adjust the Session's configuration.
pub fn start_with(fixture_name: &str, adjust: impl FnOnce(&mut SessionConfig)) -> TestSession {
    let state = tempfile::tempdir().unwrap();
    let mut config = config(fixture_name, state.path());
    adjust(&mut config);
    let session = Session::start(config);
    let events = session.events();
    TestSession {
        session,
        events,
        _state: state,
    }
}

/// A Session whose per-App state lives in `state`, so tests can share it.
pub fn start_session(fixture_name: &str, state: &std::path::Path) -> Session {
    Session::start(config(fixture_name, state))
}

fn config(fixture_name: &str, state: &std::path::Path) -> SessionConfig {
    SessionConfig {
        source: AppSource::ProjectFolder(fixture(fixture_name)),
        profile: None,
        headless: std::env::var_os("TIZEN_SIM_TEST_HEADED").is_none(),
        lenient: false,
        home: Home::from_env().with_state_dir(state.to_path_buf()),
    }
}

/// Waits for the first event matching `pred`, failing the test after `secs`.
pub async fn expect_event(
    events: &mut Events,
    secs: u64,
    pred: impl Fn(&SessionEvent) -> bool,
) -> SessionEvent {
    let wait = async {
        while let Some(event) = events.next().await {
            if pred(&event) {
                return event;
            }
        }
        panic!("event stream ended before the expected event");
    };
    tokio::time::timeout(Duration::from_secs(secs), wait)
        .await
        .expect("timed out waiting for the expected Session event")
}

/// Runs callback-style page code and returns the value it passes to `done`.
///
/// `body` is the inside of `function (done) { ... }`. Polling instead of
/// awaiting a promise keeps this working on Engines without `awaitPromise`.
pub async fn eval_callback(session: &Session, body: &str) -> Value {
    session
        .evaluate(&format!(
            "window.__testResult = undefined; (function (done) {{ {body} }})(function (v) {{ \
             window.__testResult = JSON.stringify(v === undefined ? null : v); }}); 0"
        ))
        .await
        .unwrap();
    for _ in 0..100 {
        if let Value::String(json) = session.evaluate("window.__testResult").await.unwrap() {
            return serde_json::from_str(&json).unwrap();
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("page code never called done(): {body}");
}

/// Polls a page expression until it equals `expected`.
pub async fn eventually_equals(session: &Session, expression: &str, expected: Value) {
    let mut last = Value::Null;
    for _ in 0..100 {
        if let Ok(value) = session.evaluate(expression).await {
            if value == expected {
                return;
            }
            last = value;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("`{expression}` never became {expected}; last value {last}");
}

/// First launch may include an Engine download, so it gets a generous budget.
pub const STARTUP_SECS: u64 = 600;
