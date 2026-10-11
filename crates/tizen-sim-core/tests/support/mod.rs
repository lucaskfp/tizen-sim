//! Shared harness for tests at the Session seam.
#![allow(dead_code)]

use serde_json::Value;
use std::path::PathBuf;
use std::time::Duration;
use tizen_sim_core::app::AppSource;
use tizen_sim_core::home::Home;
use tizen_sim_core::session::{Events, Lifecycle, Session, SessionConfig, SessionEvent};

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
    /// The per-App state directory, removed when the test ends.
    pub state: tempfile::TempDir,
}

pub fn start(fixture_name: &str) -> TestSession {
    start_with(fixture_name, |_| {})
}

/// Like `start`, with a chance to adjust the Session's configuration.
pub fn start_with(fixture_name: &str, adjust: impl FnOnce(&mut SessionConfig)) -> TestSession {
    start_folder(&fixture(fixture_name), adjust)
}

/// A Session on any Project Folder, e.g. a copy of a fixture a test edits.
pub fn start_folder(
    folder: &std::path::Path,
    adjust: impl FnOnce(&mut SessionConfig),
) -> TestSession {
    start_source(AppSource::ProjectFolder(folder.to_path_buf()), adjust)
}

/// A Session on any App Source.
pub fn start_source(source: AppSource, adjust: impl FnOnce(&mut SessionConfig)) -> TestSession {
    let state = tempfile::tempdir().unwrap();
    let mut config = config(source, state.path());
    adjust(&mut config);
    let session = Session::start(config);
    let events = session.events();
    TestSession {
        session,
        events,
        state,
    }
}

/// A Session whose per-App state lives in `state`, so tests can share it.
pub fn start_session(fixture_name: &str, state: &std::path::Path) -> Session {
    Session::start(config(
        AppSource::ProjectFolder(fixture(fixture_name)),
        state,
    ))
}

/// The files of a fixture App, as `(path, contents)` Package entries.
pub fn fixture_entries(fixture_name: &str) -> Vec<(String, Vec<u8>)> {
    let mut entries: Vec<(String, Vec<u8>)> = std::fs::read_dir(fixture(fixture_name))
        .unwrap()
        .map(|entry| {
            let entry = entry.unwrap();
            (
                entry.file_name().into_string().unwrap(),
                std::fs::read(entry.path()).unwrap(),
            )
        })
        .collect();
    entries.sort();
    entries
}

/// Writes a Package (a zip archive) with the given entries to `path`.
pub fn write_package(path: &std::path::Path, entries: &[(String, Vec<u8>)]) {
    use std::io::Write;
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, contents) in entries {
        zip.start_file(name.as_str(), zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(contents).unwrap();
    }
    let bytes = zip.finish().unwrap().into_inner();
    // Written in one go, as a build tool replacing the Package would.
    std::fs::write(path, bytes).unwrap();
}

/// A fixture App packed as `app.wgt` in a throwaway directory.
pub fn package_fixture(fixture_name: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let wgt = dir.path().join("app.wgt");
    write_package(&wgt, &fixture_entries(fixture_name));
    (dir, wgt)
}

/// A copy of a fixture App in a throwaway directory, for tests that edit it.
pub fn copy_fixture(fixture_name: &str) -> tempfile::TempDir {
    let copy = tempfile::tempdir().unwrap();
    for entry in std::fs::read_dir(fixture(fixture_name)).unwrap() {
        let entry = entry.unwrap();
        std::fs::copy(entry.path(), copy.path().join(entry.file_name())).unwrap();
    }
    copy
}

fn config(source: AppSource, state: &std::path::Path) -> SessionConfig {
    SessionConfig {
        source,
        profile: test_profile(),
        engine_path: None,
        // Runs the suite the way an Engine without a headless mode runs,
        // e.g. to try the virtual display such Engines need on Linux.
        engine_has_headless: std::env::var_os("TIZEN_SIM_TEST_NO_HEADLESS")
            .filter(|value| !value.is_empty())
            .map(|_| false),
        headless: std::env::var_os("TIZEN_SIM_TEST_HEADED").is_none(),
        lenient: false,
        key_map: tizen_sim_core::remote_keys::KeyMap::default(),
        device: serde_json::Map::new(),
        resolution: Default::default(),
        checkerboard: false,
        home: Home::from_env().with_state_dir(state.to_path_buf()),
    }
}

/// The Version Profile the Session tests run on: `TIZEN_SIM_TEST_PROFILE`,
/// or the latest. Run the suite once per Engine to cover each.
pub fn test_profile() -> Option<String> {
    std::env::var("TIZEN_SIM_TEST_PROFILE")
        .ok()
        .filter(|version| !version.is_empty())
}

/// The Version Profile the Session tests run on.
pub fn profile() -> &'static tizen_sim_core::profile::VersionProfile {
    match test_profile() {
        Some(version) => tizen_sim_core::profile::VersionProfile::find(&version).unwrap(),
        None => tizen_sim_core::profile::VersionProfile::latest(),
    }
}

/// A device value of the test Version Profile, as the Shims report it.
pub fn device_value(name: &str) -> Value {
    profile().device_values(&serde_json::Map::new()).unwrap()[name].clone()
}

/// Whether the test Version Profile has an API member listed in the API
/// availability table (`data/apis.toml`); unlisted members exist everywhere.
pub fn has_api(name: &str) -> bool {
    !tizen_sim_core::apis::ApiCatalog::bundled()
        .for_version(&profile().version)
        .absent
        .iter()
        .any(|absent| absent == name)
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
            // Fail fast instead of waiting out the timeout.
            if let SessionEvent::Lifecycle(Lifecycle::Failed { error }) = &event {
                panic!("the Session failed while waiting for an event: {error}");
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

/// Kills a process at once, as a crash or Task Manager would, giving it no
/// chance to clean up.
pub fn kill_process(pid: u32) {
    let status = if cfg!(windows) {
        std::process::Command::new("taskkill")
            .args(["/F", "/PID", &pid.to_string()])
            .stdout(std::process::Stdio::null())
            .status()
    } else {
        std::process::Command::new("kill")
            .args(["-9", &pid.to_string()])
            .status()
    };
    assert!(status.unwrap().success(), "could not kill process {pid}");
}

/// Whether Xvfb is installed, for an off-screen Engine on Linux.
pub fn has_xvfb() -> bool {
    std::process::Command::new("Xvfb")
        .arg("-help")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok()
}

/// The RGB colour of a pixel of a PNG screenshot.
pub fn pixel(png: &[u8], x: u32, y: u32) -> [u8; 3] {
    let decoder = png::Decoder::new(std::io::Cursor::new(png));
    let mut reader = decoder.read_info().unwrap();
    let mut buffer = vec![0; reader.output_buffer_size().unwrap()];
    let frame = reader.next_frame(&mut buffer).unwrap();
    let channels = frame.color_type.samples();
    let at = (y * frame.width + x) as usize * channels;
    [buffer[at], buffer[at + 1], buffer[at + 2]]
}
