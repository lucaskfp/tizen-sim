//! Packages at the Session seam: a `.wgt` runs like a Project Folder, and
//! replacing it unpacks it again and restarts the App.

mod support;

use serde_json::json;
use support::{
    STARTUP_SECS, eventually_equals, expect_event, fixture_entries, package_fixture, start_source,
    write_package,
};
use tizen_sim_core::app::AppSource;
use tizen_sim_core::session::{HotReload, Lifecycle, SessionEvent};

fn is_hot_reload(event: &SessionEvent, f: impl Fn(&HotReload) -> bool) -> bool {
    matches!(event, SessionEvent::HotReload(h) if f(h))
}

/// The `hot-reload` fixture's files, with `app.js` setting `scriptVersion`.
fn hot_reload_entries(script_version: u32) -> Vec<(String, Vec<u8>)> {
    let mut entries = fixture_entries("hot-reload");
    for (name, contents) in &mut entries {
        if name == "app.js" {
            *contents = format!("window.scriptVersion = {script_version};\n").into_bytes();
        }
    }
    entries
}

#[tokio::test]
async fn runs_a_package() {
    let (_dir, wgt) = package_fixture("hello");
    let mut t = start_source(AppSource::Package(wgt), |_| {});
    expect_event(&mut t.events, STARTUP_SECS, |e| {
        matches!(e, SessionEvent::Lifecycle(Lifecycle::AppLoaded { url }) if url.ends_with("/index.html"))
    })
    .await;
    t.session.wait_until_loaded().await.unwrap();
    assert_eq!(
        t.session.evaluate("document.title").await.unwrap(),
        "Hello fixture"
    );
    // The Shims are injected into a Package's pages too.
    assert_eq!(
        t.session
            .evaluate("tizen.application.getCurrentApplication().appInfo.id")
            .await
            .unwrap(),
        "Fixture001.Hello"
    );
    t.session.stop().await;
}

#[tokio::test]
async fn replacing_the_package_unpacks_it_again_and_restarts_the_app() {
    let dir = tempfile::tempdir().unwrap();
    let wgt = dir.path().join("app.wgt");
    write_package(&wgt, &hot_reload_entries(1));
    let mut t = start_source(AppSource::Package(wgt.clone()), |_| {});
    t.session.wait_until_loaded().await.unwrap();
    assert_eq!(t.session.evaluate("window.scriptVersion").await.unwrap(), 1);

    write_package(&wgt, &hot_reload_entries(2));

    expect_event(&mut t.events, 10, |e| {
        is_hot_reload(
            e,
            |h| matches!(h, HotReload::FilesChanged { paths } if paths == &["app.wgt"]),
        )
    })
    .await;
    expect_event(&mut t.events, 30, |e| {
        is_hot_reload(
            e,
            |h| matches!(h, HotReload::Restarted { app_id } if app_id == "Fixture009.HotReload"),
        )
    })
    .await;
    eventually_equals(&t.session, "window.scriptVersion", json!(2)).await;
    t.session.stop().await;
}

/// The Session directories under the test's state directory.
fn session_dirs(t: &support::TestSession) -> Vec<std::path::PathBuf> {
    std::fs::read_dir(t.state.path().join("sessions"))
        .map(|entries| entries.map(|e| e.unwrap().path()).collect())
        .unwrap_or_default()
}

#[tokio::test]
async fn a_package_is_unpacked_in_the_session_directory_and_removed_on_stop() {
    let (_dir, wgt) = package_fixture("hello");
    let mut t = start_source(AppSource::Package(wgt), |_| {});
    t.session.wait_until_loaded().await.unwrap();
    let dirs = session_dirs(&t);
    assert_eq!(dirs.len(), 1, "{dirs:?}");
    let unpacked: Vec<_> = std::fs::read_dir(&dirs[0])
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.join("index.html").is_file())
        .collect();
    assert_eq!(
        unpacked.len(),
        1,
        "no unpacked Package in {}",
        dirs[0].display()
    );

    t.session.stop().await;
    expect_event(&mut t.events, 30, |e| {
        matches!(e, SessionEvent::Lifecycle(Lifecycle::Exited { .. }))
    })
    .await;
    // The Session task tears down just after publishing Exited.
    for _ in 0..100 {
        if session_dirs(&t).is_empty() {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    panic!("left behind: {:?}", session_dirs(&t));
}

#[tokio::test]
async fn a_package_seen_mid_write_is_retried_rather_than_failing() {
    let dir = tempfile::tempdir().unwrap();
    let wgt = dir.path().join("app.wgt");
    write_package(&wgt, &hot_reload_entries(1));
    let mut t = start_source(AppSource::Package(wgt.clone()), |_| {});
    t.session.wait_until_loaded().await.unwrap();

    // A build writes the first half, pauses past the change debounce, then
    // finishes within the retry window.
    let staging = dir.path().join("staging.wgt");
    write_package(&staging, &hot_reload_entries(2));
    let complete = std::fs::read(&staging).unwrap();
    std::fs::write(&wgt, &complete[..complete.len() / 2]).unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(400)).await;
    std::fs::write(&wgt, &complete).unwrap();

    let outcome = expect_event(&mut t.events, 30, |e| {
        is_hot_reload(e, |h| {
            matches!(h, HotReload::Restarted { .. } | HotReload::Failed { .. })
        })
    })
    .await;
    if let SessionEvent::HotReload(HotReload::Failed { error }) = outcome {
        panic!("a half-written Package raised a failure: {error}");
    }
    eventually_equals(&t.session, "window.scriptVersion", json!(2)).await;
    t.session.stop().await;
}

#[tokio::test]
async fn a_corrupt_replacement_keeps_the_running_app() {
    let dir = tempfile::tempdir().unwrap();
    let wgt = dir.path().join("app.wgt");
    write_package(&wgt, &hot_reload_entries(1));
    let mut t = start_source(AppSource::Package(wgt.clone()), |_| {});
    t.session.wait_until_loaded().await.unwrap();

    std::fs::write(&wgt, b"not a zip archive").unwrap();

    let failed = expect_event(&mut t.events, 30, |e| {
        is_hot_reload(e, |h| matches!(h, HotReload::Failed { .. }))
    })
    .await;
    let SessionEvent::HotReload(HotReload::Failed { error }) = failed else {
        unreachable!()
    };
    assert!(error.contains("not a valid Package"), "{error}");
    // A fresh load is still served from the previous Package's files.
    t.session
        .evaluate("window.beforeReload = true; location.reload(); 0")
        .await
        .unwrap();
    eventually_equals(
        &t.session,
        "document.readyState === 'complete' && !window.beforeReload && window.scriptVersion",
        json!(1),
    )
    .await;
    t.session.stop().await;
}

#[tokio::test]
async fn a_corrupt_package_fails_to_start_with_the_problem_named() {
    let dir = tempfile::tempdir().unwrap();
    let wgt = dir.path().join("broken.wgt");
    std::fs::write(&wgt, b"not a zip archive").unwrap();
    let mut t = start_source(AppSource::Package(wgt), |_| {});
    let failed = expect_event(&mut t.events, 10, |e| {
        matches!(e, SessionEvent::Lifecycle(Lifecycle::Failed { .. }))
    })
    .await;
    let SessionEvent::Lifecycle(Lifecycle::Failed { error }) = failed else {
        unreachable!()
    };
    assert!(
        error.contains("broken.wgt") && error.contains("not a valid Package"),
        "{error}"
    );
}
