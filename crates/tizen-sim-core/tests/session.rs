//! Primary seam: a Session on a real Engine (Chrome for Testing M130).

mod support;

use support::{STARTUP_SECS, expect_event, start};
use tizen_sim_core::session::{ExitReason, Lifecycle, SessionEvent};

fn is_lifecycle(event: &SessionEvent, f: impl Fn(&Lifecycle) -> bool) -> bool {
    matches!(event, SessionEvent::Lifecycle(l) if f(l))
}

#[tokio::test]
async fn runs_a_project_folder_and_streams_lifecycle_and_console() {
    let mut t = start("hello");

    expect_event(&mut t.events, 5, |e| {
        is_lifecycle(e, |l| {
            matches!(l, Lifecycle::Starting { app_id, profile }
                if app_id == "Fixture001.Hello" && profile == "10.0")
        })
    })
    .await;
    expect_event(&mut t.events, STARTUP_SECS, |e| {
        is_lifecycle(
            e,
            |l| matches!(l, Lifecycle::EngineReady { engine_version, .. } if engine_version.contains("/130.")),
        )
    })
    .await;
    expect_event(&mut t.events, 30, |e| {
        is_lifecycle(
            e,
            |l| matches!(l, Lifecycle::AppLoaded { url } if url.ends_with("/index.html")),
        )
    })
    .await;
    t.session.wait_until_loaded().await.unwrap();

    // Console output from the App's very first script is not lost.
    let mut console = t.session.events();
    expect_event(&mut console, 10, |e| {
        matches!(e, SessionEvent::Console(m) if m.text == "hello from fixture" && m.level == "log")
    })
    .await;
    expect_event(&mut console, 10, |e| {
        matches!(e, SessionEvent::Console(m) if m.text == "a warning from fixture" && m.level == "warning")
    })
    .await;

    let title = t.session.evaluate("document.title").await.unwrap();
    assert_eq!(title, "Hello fixture");

    t.session.stop().await;
    expect_event(&mut t.events, 30, |e| {
        is_lifecycle(e, |l| {
            matches!(
                l,
                Lifecycle::Exited {
                    reason: ExitReason::Stopped
                }
            )
        })
    })
    .await;
}

#[tokio::test]
async fn stop_leaves_no_engine_or_server_running() {
    let mut t = start("hello");
    t.session.wait_until_loaded().await.unwrap();
    let SessionEvent::Lifecycle(Lifecycle::EngineReady { debugger_url, .. }) =
        expect_event(&mut t.events, 5, |e| {
            is_lifecycle(e, |l| matches!(l, Lifecycle::EngineReady { .. }))
        })
        .await
    else {
        unreachable!()
    };
    let SessionEvent::Lifecycle(Lifecycle::AppLoaded { url }) =
        expect_event(&mut t.events, 5, |e| {
            is_lifecycle(e, |l| matches!(l, Lifecycle::AppLoaded { .. }))
        })
        .await
    else {
        unreachable!()
    };

    t.session.stop().await;

    let host = |u: &str| {
        u.split("://")
            .nth(1)
            .unwrap()
            .split('/')
            .next()
            .unwrap()
            .to_string()
    };
    for addr in [host(&debugger_url), host(&url)] {
        assert!(
            tokio::net::TcpStream::connect(&addr).await.is_err(),
            "{addr} still accepts connections after stop"
        );
    }
}

#[tokio::test]
async fn a_second_session_reuses_the_cached_engine() {
    let first = start("hello");
    first.session.wait_until_loaded().await.unwrap();
    first.session.stop().await;

    let mut second = start("hello");
    second.session.wait_until_loaded().await.unwrap();
    second.session.stop().await;
    let mut downloads = 0;
    while let Ok(Some(event)) =
        tokio::time::timeout(std::time::Duration::from_millis(200), second.events.next()).await
    {
        if matches!(event, SessionEvent::EngineDownload(_)) {
            downloads += 1;
        }
    }
    assert_eq!(downloads, 0, "the Engine was downloaded again");
}

#[tokio::test]
async fn the_same_app_and_profile_cannot_run_twice_at_once() {
    let state = tempfile::tempdir().unwrap();
    let first = support::start_session("hello", state.path());
    first.wait_until_loaded().await.unwrap();

    let second = support::start_session("hello", state.path());
    let err = second.wait_until_loaded().await.unwrap_err();
    assert!(err.to_string().contains("already running"), "{err}");

    // The first Session is unaffected.
    assert_eq!(
        first.evaluate("document.title").await.unwrap(),
        "Hello fixture"
    );
    first.stop().await;
}

#[tokio::test]
async fn invalid_config_fails_the_session_with_the_problem_named() {
    let mut t = start("no-application");
    let err = t.session.wait_until_loaded().await.unwrap_err();
    assert!(err.to_string().contains("tizen:application"), "{err}");
    expect_event(&mut t.events, 5, |e| {
        is_lifecycle(
            e,
            |l| matches!(l, Lifecycle::Failed { error } if error.contains("tizen:application")),
        )
    })
    .await;
    // Exited is always the last lifecycle event, even after a failed start.
    expect_event(&mut t.events, 5, |e| {
        is_lifecycle(e, |l| {
            matches!(
                l,
                Lifecycle::Exited {
                    reason: ExitReason::FailedToStart
                }
            )
        })
    })
    .await;
}

/// Installation fails when the TV's Tizen version is lower than the App's
/// `required_version` (TD guides/concepts/web-runtime.md).
#[tokio::test]
async fn an_app_requiring_a_newer_tizen_fails_to_start() {
    let t = start("required-version-too-high");
    let err = t.session.wait_until_loaded().await.unwrap_err();
    let msg = err.to_string();
    for word in ["required_version", "99.0", "10.0"] {
        assert!(msg.contains(word), "`{word}` missing from: {msg}");
    }
}

#[tokio::test]
async fn an_engine_path_is_launched_instead_of_a_cached_engine() {
    // The pinned Engine's binary, used as if it were the developer's own.
    let home = tizen_sim_core::home::Home::from_env();
    let platform = tizen_sim_core::engine::current_platform().unwrap();
    let spec = tizen_sim_core::engine::EngineSpec::for_profile("10.0", platform).unwrap();
    let own = home.engine_store().ensure(spec, |_| {}).await.unwrap();
    let empty_home = tempfile::tempdir().unwrap();

    let mut t = support::start_with("hello", |config| {
        config.home = tizen_sim_core::home::Home::at(empty_home.path());
        config.engine_path = Some(own.executable.clone());
    });
    t.session.wait_until_loaded().await.unwrap();
    t.session.stop().await;

    let mut downloads = 0;
    while let Ok(Some(event)) =
        tokio::time::timeout(std::time::Duration::from_millis(200), t.events.next()).await
    {
        if matches!(event, SessionEvent::EngineDownload(_)) {
            downloads += 1;
        }
    }
    assert_eq!(downloads, 0, "an Engine was downloaded");
    assert!(
        !empty_home.path().join("engines").exists(),
        "the Engine cache was used"
    );
}

#[tokio::test]
async fn a_session_remembers_its_profile_for_the_app() {
    let t = start("hello");
    t.session.wait_until_loaded().await.unwrap();
    t.session.stop().await;

    let remembered = tizen_sim_core::profile::RememberedProfiles::new(t.state.path());
    let latest = &tizen_sim_core::profile::VersionProfile::latest().version;
    assert_eq!(remembered.get("Fixture001.Hello").as_ref(), Some(latest));
}

#[tokio::test]
async fn an_engine_that_dies_ends_the_session_as_crashed() {
    let mut t = start("hello");
    t.session.wait_until_loaded().await.unwrap();
    let pid = t
        .session
        .engine_pid()
        .expect("a running Engine has a process");

    support::kill_process(pid);

    expect_event(&mut t.events, 30, |e| {
        is_lifecycle(e, |l| {
            matches!(
                l,
                Lifecycle::Exited {
                    reason: ExitReason::EngineCrashed
                }
            )
        })
    })
    .await;
    assert_eq!(t.session.engine_pid(), None);
}
