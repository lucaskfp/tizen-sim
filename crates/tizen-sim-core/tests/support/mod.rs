//! Shared harness for tests at the Session seam.
#![allow(dead_code)]

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
    let state = tempfile::tempdir().unwrap();
    let session = start_session(fixture_name, state.path());
    let events = session.events();
    TestSession {
        session,
        events,
        _state: state,
    }
}

/// A Session whose per-App state lives in `state`, so tests can share it.
pub fn start_session(fixture_name: &str, state: &std::path::Path) -> Session {
    let home = Home::from_env().with_state_dir(state.to_path_buf());
    Session::start(SessionConfig {
        source: AppSource::ProjectFolder(fixture(fixture_name)),
        profile: None,
        headless: std::env::var_os("TIZEN_SIM_TEST_HEADED").is_none(),
        home,
    })
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

/// First launch may include an Engine download, so it gets a generous budget.
pub const STARTUP_SECS: u64 = 600;
