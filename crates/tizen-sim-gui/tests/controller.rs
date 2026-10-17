//! The GUI's Session control, driven as its commands drive it, against real
//! headless Sessions on fixture Apps.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tizen_sim_core::app::AppSource;
use tizen_sim_core::home::Home;
use tizen_sim_core::registry::{ChooseError, RegistryEntry, SessionRegistry};
use tizen_sim_core::session::{
    ConsoleMessage, ExitReason, KeyOutcome, Lifecycle, Session, SessionConfig, SessionEvent,
    SessionStatus,
};
use tizen_sim_gui::controller::{Controller, ControllerError, ControllerOptions, GuiEvent, Screen};
use tokio::sync::mpsc;

fn fixture(name: &str) -> AppSource {
    AppSource::ProjectFolder(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/apps")
            .join(name),
    )
}

/// What the Controller asked of the GUI's screen area.
#[derive(Debug, Clone, PartialEq)]
enum ScreenCall {
    Embed { window: u64, owned: bool },
    Release,
}

/// A screen area that records what it is asked and embeds any window.
#[derive(Default)]
struct FakeScreen {
    calls: Mutex<Vec<ScreenCall>>,
}

impl FakeScreen {
    fn calls(&self) -> Vec<ScreenCall> {
        self.calls.lock().unwrap().clone()
    }
}

impl Screen for FakeScreen {
    fn embed(&self, window: u64, _engine_pid: Option<u32>, owned: bool) -> bool {
        self.calls
            .lock()
            .unwrap()
            .push(ScreenCall::Embed { window, owned });
        true
    }

    fn release(&self) {
        self.calls.lock().unwrap().push(ScreenCall::Release);
    }
}

/// A Controller whose events land in `events`. Engines come from the shared
/// per-user cache; per-App state lives in `state`.
struct TestGui {
    controller: Controller,
    events: mpsc::UnboundedReceiver<GuiEvent>,
    screen: Arc<FakeScreen>,
    home: Home,
    _state: tempfile::TempDir,
}

fn headless() -> bool {
    std::env::var_os("TIZEN_SIM_TEST_HEADED").is_none()
}

fn gui() -> TestGui {
    let state = tempfile::tempdir().unwrap();
    let home = Home::from_env().with_state_dir(state.path().to_path_buf());
    let (tx, events) = mpsc::unbounded_channel();
    let screen = Arc::new(FakeScreen::default());
    let controller = Controller::new(
        ControllerOptions {
            home: home.clone(),
            headless: headless(),
            engine_path: None,
        },
        move |event| {
            let _ = tx.send(event);
        },
    )
    .with_screen(screen.clone());
    TestGui {
        controller,
        events,
        screen,
        home,
        _state: state,
    }
}

/// A Session the GUI doesn't own, as `tizen-sim run` starts it. It shares
/// the GUI's per-user state, and so its Session Registry.
async fn start_elsewhere(gui: &TestGui, name: &str) -> (Session, RegistryEntry) {
    let session = Session::start(SessionConfig {
        source: fixture(name),
        profile: None,
        engine_path: None,
        engine_has_headless: None,
        headless: headless(),
        lenient: false,
        key_map: Default::default(),
        device: Default::default(),
        resolution: Default::default(),
        checkerboard: false,
        home: gui.home.clone(),
    });
    session.wait_until_loaded().await.unwrap();
    let entry = only_entry(&gui.home);
    (session, entry)
}

fn only_entry(home: &Home) -> RegistryEntry {
    let entries = SessionRegistry::new(home).list();
    assert_eq!(entries.len(), 1, "{entries:#?}");
    entries.into_iter().next().unwrap()
}

/// Kills a process at once, as a crash would.
fn kill(pid: u32) {
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

fn is_exit(event: &GuiEvent, session: u64, reason: ExitReason) -> bool {
    matches!(event, GuiEvent::Session {
        session: s,
        event: SessionEvent::Lifecycle(Lifecycle::Exited { reason: r }),
    } if *s == session && *r == reason)
}

fn is_screen(event: &GuiEvent, session: u64) -> bool {
    matches!(event, GuiEvent::Screen { session: s, .. } if *s == session)
}

/// First launch may include an Engine download, so it gets a generous budget.
const STARTUP_SECS: u64 = 600;

impl TestGui {
    /// Waits for the first event matching `pred`, failing after `secs` and
    /// naming the events that came instead, like the core's `expect_event`.
    async fn expect(&mut self, secs: u64, pred: impl Fn(&GuiEvent) -> bool) -> GuiEvent {
        let mut seen = Vec::new();
        let wait = async {
            while let Some(event) = self.events.recv().await {
                if pred(&event) {
                    return event;
                }
                seen.push(format!("{event:?}"));
            }
            panic!("the event stream ended before the expected event");
        };
        let result = tokio::time::timeout(Duration::from_secs(secs), wait).await;
        result.unwrap_or_else(|_| {
            panic!(
                "timed out after {secs}s waiting for the expected GUI event; saw instead: {seen:#?}"
            )
        })
    }

    /// Waits until Session `session` has loaded its App.
    async fn expect_loaded(&mut self, session: u64) {
        self.expect(STARTUP_SECS, |e| {
            matches!(e, GuiEvent::Session { session: s, event: SessionEvent::Lifecycle(Lifecycle::AppLoaded { .. }) } if *s == session)
        })
        .await;
    }
}

#[tokio::test]
async fn opening_a_project_folder_starts_a_session_and_streams_its_console() {
    let mut gui = gui();

    gui.controller.open(fixture("hello"), None).await;

    let started = gui
        .expect(5, |e| matches!(e, GuiEvent::SessionStarted { .. }))
        .await;
    let GuiEvent::SessionStarted { session, .. } = started else {
        unreachable!()
    };
    gui.expect(STARTUP_SECS, |e| {
        matches!(e, GuiEvent::Session {
            session: s,
            event: SessionEvent::Console(ConsoleMessage { text, .. }),
        } if *s == session && text == "hello from fixture")
    })
    .await;
    gui.controller.stop().await;
}

#[tokio::test]
async fn the_remote_delivers_keys_and_dropped_keys_show_in_the_stream() {
    let mut gui = gui();
    gui.controller.open(fixture("remote-keys"), None).await;
    gui.expect_loaded(1).await;

    let enter = gui.controller.press_key("Enter").await.unwrap();
    let red = gui.controller.press_key("ColorF0Red").await.unwrap();

    assert_eq!(enter, KeyOutcome::Delivered);
    assert!(matches!(red, KeyOutcome::Dropped { .. }), "{red:?}");
    let dropped = gui
        .expect(5, |e| {
            matches!(
                e,
                GuiEvent::Session {
                    event: SessionEvent::RemoteKey {
                        outcome: KeyOutcome::Dropped { .. },
                        ..
                    },
                    ..
                }
            )
        })
        .await;
    let GuiEvent::Session {
        event: SessionEvent::RemoteKey { key, .. },
        ..
    } = dropped
    else {
        unreachable!()
    };
    assert_eq!(key, "ColorF0Red");
    gui.controller.stop().await;
}

#[tokio::test]
async fn acting_without_a_session_is_an_error() {
    let gui = gui();

    let error = gui.controller.press_key("Enter").await.unwrap_err();

    assert!(matches!(error, ControllerError::NoSession), "{error}");
}

#[tokio::test]
async fn switching_version_profile_restarts_the_session_on_that_profile() {
    let mut gui = gui();
    gui.controller.open(fixture("hello"), None).await;
    gui.expect_loaded(1).await;

    gui.controller.switch_profile("10.0").await.unwrap();

    gui.expect(30, |e| {
        matches!(
            e,
            GuiEvent::Session {
                session: 1,
                event: SessionEvent::Lifecycle(Lifecycle::Exited {
                    reason: ExitReason::Stopped
                })
            }
        )
    })
    .await;
    let started = gui
        .expect(5, |e| matches!(e, GuiEvent::SessionStarted { .. }))
        .await;
    assert!(
        matches!(&started, GuiEvent::SessionStarted { session: 2, source, adopted: false } if source.ends_with("hello")),
        "{started:?}"
    );
    gui.expect(STARTUP_SECS, |e| {
        matches!(e, GuiEvent::Session { session: 2, event: SessionEvent::Lifecycle(Lifecycle::Starting { profile, .. }) } if profile == "10.0")
    })
    .await;
    gui.expect_loaded(2).await;
    gui.controller.stop().await;
}

#[tokio::test]
async fn changing_a_device_value_restarts_the_session_with_it() {
    let mut gui = gui();
    gui.controller.open(fixture("hello"), None).await;
    gui.expect_loaded(1).await;
    assert_eq!(
        gui.controller.device_values("10.0").unwrap()["energySaving"],
        "OFF"
    );

    gui.controller
        .set_device_value("energySaving", serde_json::json!("HIGH"))
        .await
        .unwrap();

    gui.expect(30, |e| is_exit(e, 1, ExitReason::Stopped)).await;
    gui.expect_loaded(2).await;
    // The GUI's choice holds on every Version Profile.
    for profile in ["10.0", "3.0"] {
        assert_eq!(
            gui.controller.device_values(profile).unwrap()["energySaving"],
            "HIGH"
        );
    }
    gui.controller.stop().await;
}

#[tokio::test]
async fn an_unknown_device_value_is_refused() {
    let gui = gui();
    gui.controller.open(fixture("hello"), None).await;

    let error = gui
        .controller
        .set_device_value("energySavings", serde_json::json!("HIGH"))
        .await
        .unwrap_err();

    assert!(error.to_string().contains("energySavings"), "{error}");
    // Nothing was kept that would fail the next start.
    assert!(gui.controller.device_values("10.0").is_ok());
    gui.controller.stop().await;
}

#[tokio::test]
async fn switching_to_an_unknown_version_profile_reports_why_the_session_failed() {
    let mut gui = gui();
    gui.controller.open(fixture("hello"), None).await;
    gui.expect_loaded(1).await;

    gui.controller.switch_profile("2.4").await.unwrap();

    let failed = gui
        .expect(30, |e| {
            matches!(
                e,
                GuiEvent::Session {
                    session: 2,
                    event: SessionEvent::Lifecycle(Lifecycle::Failed { .. })
                }
            )
        })
        .await;
    let GuiEvent::Session {
        event: SessionEvent::Lifecycle(Lifecycle::Failed { error }),
        ..
    } = failed
    else {
        unreachable!()
    };
    assert!(error.contains("2.4") && error.contains("10.0"), "{error}");
}

#[tokio::test]
async fn reload_and_screenshot_act_on_the_current_session() {
    let mut gui = gui();
    gui.controller.open(fixture("hello"), None).await;
    gui.expect_loaded(1).await;

    gui.controller.reload().await.unwrap();
    let png = gui.controller.screenshot().await.unwrap();

    gui.expect(5, |e| {
        matches!(
            e,
            GuiEvent::Session {
                session: 1,
                event: SessionEvent::Lifecycle(Lifecycle::Reloaded)
            }
        )
    })
    .await;
    assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"), "not a PNG");
    gui.controller.stop().await;
}

#[tokio::test]
async fn a_loaded_app_goes_on_the_screen_if_its_engine_has_a_window() {
    let mut gui = gui();
    gui.controller.open(fixture("hello"), None).await;
    gui.expect_loaded(1).await;

    let screen = gui.expect(10, |e| is_screen(e, 1)).await;

    let entry = only_entry(&gui.home);
    // Only a windowed Engine on Windows has a window to embed (ADR-0006).
    assert!(
        matches!(screen, GuiEvent::Screen { embedded, .. } if embedded == entry.window.is_some()),
        "{screen:?}"
    );
    match entry.window {
        Some(window) => assert_eq!(
            gui.screen.calls(),
            [ScreenCall::Embed {
                window,
                owned: true
            }]
        ),
        None => assert_eq!(gui.screen.calls(), []),
    }
    gui.controller.stop().await;
}

#[tokio::test]
async fn a_session_started_elsewhere_can_be_adopted_and_is_left_running() {
    let mut gui = gui();
    let (cli, entry) = start_elsewhere(&gui, "remote-keys").await;

    gui.controller.adopt(&entry.id).await.unwrap();

    let started = gui
        .expect(5, |e| matches!(e, GuiEvent::SessionStarted { .. }))
        .await;
    assert!(
        matches!(&started, GuiEvent::SessionStarted { session: 1, source, adopted: true } if source.ends_with("remote-keys")),
        "{started:?}"
    );
    // Its events so far come first, then new ones.
    gui.expect_loaded(1).await;
    let screen = gui.expect(10, |e| is_screen(e, 1)).await;
    assert!(
        matches!(screen, GuiEvent::Screen { embedded, .. } if embedded == entry.window.is_some()),
        "{screen:?}"
    );
    let enter = gui.controller.press_key("Enter").await.unwrap();
    assert_eq!(enter, KeyOutcome::Delivered);
    gui.expect(5, |e| {
        matches!(e, GuiEvent::Session { session: 1, event: SessionEvent::RemoteKey { key, outcome: KeyOutcome::Delivered } } if key == "Enter")
    })
    .await;
    let png = gui.controller.screenshot().await.unwrap();
    assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"), "not a PNG");
    // Its owner chose its device values.
    let error = gui
        .controller
        .set_device_value("energySaving", serde_json::json!("HIGH"))
        .await
        .unwrap_err();
    assert!(matches!(error, ControllerError::Adopted), "{error}");

    // Letting go of it gives its window back and leaves it running.
    gui.controller.stop().await;

    assert_eq!(cli.status(), SessionStatus::Loaded);
    let calls = gui.screen.calls();
    assert_eq!(calls.last(), Some(&ScreenCall::Release), "{calls:?}");
    if let Some(window) = entry.window {
        assert_eq!(
            calls[0],
            ScreenCall::Embed {
                window,
                owned: false
            }
        );
    }
    cli.stop().await;
}

#[tokio::test]
async fn the_session_the_gui_shows_is_not_adopted_again() {
    let mut gui = gui();
    gui.controller.open(fixture("hello"), None).await;
    gui.expect_loaded(1).await;
    let own = only_entry(&gui.home);

    let error = gui.controller.adopt(&own.id).await.unwrap_err();

    assert!(matches!(error, ControllerError::AlreadyShown), "{error}");
    // It keeps running.
    gui.controller.reload().await.unwrap();
    gui.controller.stop().await;
}

#[tokio::test]
async fn adopting_a_session_that_is_not_running_is_an_error() {
    let gui = gui();

    let error = gui.controller.adopt("nope").await.unwrap_err();

    assert!(
        matches!(&error, ControllerError::Registry(ChooseError::NotFound(id)) if id == "nope"),
        "{error}"
    );
}

#[tokio::test]
async fn a_crashed_engine_shows_as_crashed_and_restart_starts_the_app_again() {
    let mut gui = gui();
    gui.controller.open(fixture("hello"), None).await;
    gui.expect_loaded(1).await;
    let entry = only_entry(&gui.home);

    kill(entry.engine_pid.unwrap());

    gui.expect(30, |e| is_exit(e, 1, ExitReason::EngineCrashed))
        .await;
    gui.controller.restart().await.unwrap();
    let started = gui
        .expect(5, |e| matches!(e, GuiEvent::SessionStarted { .. }))
        .await;
    assert!(
        matches!(&started, GuiEvent::SessionStarted { session: 2, source, adopted: false } if source.ends_with("hello")),
        "{started:?}"
    );
    gui.expect_loaded(2).await;
    gui.controller.stop().await;
}

#[tokio::test]
async fn an_adopted_session_whose_engine_crashed_is_restarted_by_the_gui() {
    let mut gui = gui();
    let (cli, entry) = start_elsewhere(&gui, "hello").await;
    gui.controller.adopt(&entry.id).await.unwrap();
    gui.expect_loaded(1).await;

    kill(entry.engine_pid.unwrap());

    gui.expect(30, |e| is_exit(e, 1, ExitReason::EngineCrashed))
        .await;
    cli.stop().await;
    gui.controller.restart().await.unwrap();
    let started = gui
        .expect(5, |e| matches!(e, GuiEvent::SessionStarted { .. }))
        .await;
    assert!(
        matches!(
            &started,
            GuiEvent::SessionStarted {
                session: 2,
                adopted: false,
                ..
            }
        ),
        "{started:?}"
    );
    gui.expect_loaded(2).await;
    gui.controller.stop().await;
}
