//! Hot reload of Project Folders, at the Session seam: tests edit a copy of
//! the `hot-reload` fixture while its Session runs.

mod support;

use serde_json::json;
use std::path::Path;
use std::time::Duration;
use support::{copy_fixture, eventually_equals, expect_event, start_folder};
use tizen_sim_core::session::{Events, HotReload, SessionEvent};

const PRIVILEGED_CONFIG: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<widget xmlns="http://www.w3.org/ns/widgets" xmlns:tizen="http://tizen.org/ns/widgets" id="http://example.com/hot-reload" version="1.0.0">
    <tizen:application id="Fixture009.HotReload" package="Fixture009" required_version="2.3"/>
    <content src="index.html"/>
    <name>Hot reload fixture</name>
    <tizen:profile name="tv"/>
    <tizen:privilege name="http://tizen.org/privilege/application.info"/>
</widget>
"#;

fn write(folder: &Path, file: &str, contents: &str) {
    std::fs::write(folder.join(file), contents).unwrap();
}

fn is_hot_reload(event: &SessionEvent, f: impl Fn(&HotReload) -> bool) -> bool {
    matches!(event, SessionEvent::HotReload(h) if f(h))
}

/// Counts the hot reload events matching `pred` until the stream is quiet
/// for `quiet`.
async fn count_until_quiet(
    events: &mut Events,
    quiet: Duration,
    pred: impl Fn(&HotReload) -> bool,
) -> usize {
    let mut count = 0;
    while let Ok(Some(event)) = tokio::time::timeout(quiet, events.next()).await {
        if is_hot_reload(&event, &pred) {
            count += 1;
        }
    }
    count
}

#[tokio::test]
async fn editing_a_script_reloads_the_app_and_runs_the_new_code() {
    let folder = copy_fixture("hot-reload");
    let mut t = start_folder(folder.path(), |_| {});
    t.session.wait_until_loaded().await.unwrap();
    assert_eq!(t.session.evaluate("window.scriptVersion").await.unwrap(), 1);

    write(folder.path(), "app.js", "window.scriptVersion = 2;\n");

    expect_event(&mut t.events, 10, |e| {
        is_hot_reload(
            e,
            |h| matches!(h, HotReload::FilesChanged { paths } if paths == &["app.js"]),
        )
    })
    .await;
    expect_event(&mut t.events, 30, |e| {
        is_hot_reload(e, |h| matches!(h, HotReload::Reloaded))
    })
    .await;
    eventually_equals(&t.session, "window.scriptVersion", json!(2)).await;
    t.session.stop().await;
}

#[tokio::test]
async fn editing_a_stylesheet_swaps_it_in_place_without_reloading() {
    let folder = copy_fixture("hot-reload");
    let mut t = start_folder(folder.path(), |_| {});
    t.session.wait_until_loaded().await.unwrap();
    let color = "getComputedStyle(document.body).color";
    eventually_equals(&t.session, color, json!("rgb(1, 1, 1)")).await;
    // Page state that a reload would lose.
    t.session
        .evaluate("window.pageState = 'kept'; 0")
        .await
        .unwrap();

    write(
        folder.path(),
        "style.css",
        "body { color: rgb(2, 2, 2); }\n",
    );

    expect_event(&mut t.events, 30, |e| {
        is_hot_reload(e, |h| matches!(h, HotReload::StylesSwapped))
    })
    .await;
    eventually_equals(&t.session, color, json!("rgb(2, 2, 2)")).await;
    assert_eq!(
        t.session.evaluate("window.pageState").await.unwrap(),
        "kept"
    );
    let reloads = count_until_quiet(&mut t.events, Duration::from_millis(500), |h| {
        matches!(h, HotReload::Reloaded)
    })
    .await;
    assert_eq!(reloads, 0, "a stylesheet change reloaded the App");
    t.session.stop().await;
}

/// `application.info` guards ApplicationInformation.size
/// (application.html#ApplicationInformation::size), so declaring it in
/// `config.xml` shows the Session re-read the privileges.
#[tokio::test]
async fn editing_config_xml_restarts_the_session_with_the_new_privileges() {
    let folder = copy_fixture("hot-reload");
    let mut t = start_folder(folder.path(), |_| {});
    t.session.wait_until_loaded().await.unwrap();
    let read_size = "(function () { try { return typeof tizen.application.getAppInfo().size; } \
                     catch (e) { return e.name; } })()";
    assert_eq!(
        t.session.evaluate(read_size).await.unwrap(),
        "SecurityError"
    );

    write(folder.path(), "config.xml", PRIVILEGED_CONFIG);

    expect_event(&mut t.events, 30, |e| {
        is_hot_reload(
            e,
            |h| matches!(h, HotReload::Restarted { app_id } if app_id == "Fixture009.HotReload"),
        )
    })
    .await;
    eventually_equals(&t.session, read_size, json!("number")).await;
    t.session.stop().await;
}

/// While the developer is mid-edit, a `config.xml` that no longer loads keeps
/// the App running as it was and says why.
#[tokio::test]
async fn a_broken_config_xml_leaves_the_app_running() {
    let folder = copy_fixture("hot-reload");
    let mut t = start_folder(folder.path(), |_| {});
    t.session.wait_until_loaded().await.unwrap();
    t.session
        .evaluate("window.pageState = 'kept'; 0")
        .await
        .unwrap();

    write(folder.path(), "config.xml", "<widget");

    expect_event(&mut t.events, 30, |e| {
        is_hot_reload(
            e,
            |h| matches!(h, HotReload::Failed { error } if error.contains("config.xml")),
        )
    })
    .await;
    assert_eq!(
        t.session.evaluate("window.pageState").await.unwrap(),
        "kept"
    );
    t.session.stop().await;
}

#[tokio::test]
async fn a_burst_of_changes_reloads_once() {
    let folder = copy_fixture("hot-reload");
    let mut t = start_folder(folder.path(), |_| {});
    t.session.wait_until_loaded().await.unwrap();

    // An editor's save-all writes several files in quick succession.
    write(folder.path(), "app.js", "window.scriptVersion = 3;\n");
    tokio::time::sleep(Duration::from_millis(20)).await;
    write(
        folder.path(),
        "style.css",
        "body { color: rgb(3, 3, 3); }\n",
    );
    tokio::time::sleep(Duration::from_millis(20)).await;
    write(folder.path(), "extra.js", "window.extra = true;\n");
    write(folder.path(), "app.js", "window.scriptVersion = 4;\n");

    expect_event(&mut t.events, 30, |e| {
        is_hot_reload(e, |h| matches!(h, HotReload::Reloaded))
    })
    .await;
    let more = count_until_quiet(&mut t.events, Duration::from_millis(1500), |h| {
        matches!(h, HotReload::Reloaded | HotReload::StylesSwapped)
    })
    .await;
    assert_eq!(more, 0, "the burst caused more than one reload");
    eventually_equals(&t.session, "window.scriptVersion", json!(4)).await;
    t.session.stop().await;
}

/// The Engine's data belongs to the App id, so a `config.xml` naming another
/// App can't take over this Session.
#[tokio::test]
async fn changing_the_application_id_is_refused() {
    let folder = copy_fixture("hot-reload");
    let mut t = start_folder(folder.path(), |_| {});
    t.session.wait_until_loaded().await.unwrap();

    let renamed = PRIVILEGED_CONFIG.replace("Fixture009.HotReload", "Fixture009.Renamed");
    write(folder.path(), "config.xml", &renamed);

    expect_event(&mut t.events, 30, |e| {
        is_hot_reload(
            e,
            |h| matches!(h, HotReload::Failed { error } if error.contains("Fixture009.Renamed")),
        )
    })
    .await;
    t.session.stop().await;
}
