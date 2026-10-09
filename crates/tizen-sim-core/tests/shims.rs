//! The Shim bootstrap and the first Tizen API Shims, exercised through a
//! Session on a real Engine.

mod support;

use serde_json::json;
use support::{eval_callback, eventually_equals, expect_event, start, start_with};
use tizen_sim_core::session::{ExitReason, Lifecycle, SessionEvent};

const APP_INFO_PRIVILEGE: &str = "http://tizen.org/privilege/application.info";

#[tokio::test]
async fn tizen_exists_before_the_first_script_on_every_page() {
    let t = start("tizen-apis");
    t.session.wait_until_loaded().await.unwrap();
    let saw = t
        .session
        .evaluate("window.firstScriptSawTizen")
        .await
        .unwrap();
    assert_eq!(saw, "object");

    t.session
        .evaluate("location.href = 'other.html'; 0")
        .await
        .unwrap();
    eventually_equals(
        &t.session,
        "location.pathname.indexOf('other.html') >= 0 && window.firstScriptSawTizen",
        json!("object"),
    )
    .await;
    t.session.stop().await;
}

#[tokio::test]
async fn systeminfo_reports_version_profile_values() {
    let t = start("tizen-apis");
    t.session.wait_until_loaded().await.unwrap();

    let version = t
        .session
        .evaluate("tizen.systeminfo.getCapability('http://tizen.org/feature/platform.version')")
        .await
        .unwrap();
    assert_eq!(version, "10.0");

    let display = eval_callback(
        &t.session,
        "tizen.systeminfo.getPropertyValue('DISPLAY', function (d) { \
           done([d.resolutionWidth, d.resolutionHeight]); }, function (e) { done(e.name); });",
    )
    .await;
    assert_eq!(display, json!([1920, 1080]));

    let build = eval_callback(
        &t.session,
        "tizen.systeminfo.getPropertyValue('BUILD', function (b) { done(b.model); }, \
           function (e) { done('error ' + e.name); });",
    )
    .await;
    assert_eq!(build, "TIZEN-SIM-TV");

    let unsupported = eval_callback(
        &t.session,
        "tizen.systeminfo.getPropertyValue('NOT_A_PROPERTY', function () { done('success'); }, \
           function (e) { done(e.name); });",
    )
    .await;
    assert_eq!(unsupported, "NotSupportedError");
    t.session.stop().await;
}

#[tokio::test]
async fn application_describes_the_running_app() {
    let t = start("tizen-apis");
    t.session.wait_until_loaded().await.unwrap();
    let info = t
        .session
        .evaluate(
            "(function () { var i = tizen.application.getCurrentApplication().appInfo; \
               return [i.id, i.name, i.version, i.packageId]; })()",
        )
        .await
        .unwrap();
    assert_eq!(
        info,
        json!([
            "Fixture004.TizenApis",
            "Tizen APIs fixture",
            "1.2.3",
            "Fixture004"
        ])
    );
    t.session.stop().await;
}

#[tokio::test]
async fn exit_ends_the_session() {
    let mut t = start("tizen-apis");
    t.session.wait_until_loaded().await.unwrap();
    t.session
        .evaluate("tizen.application.getCurrentApplication().exit(); 0")
        .await
        .unwrap();
    expect_event(&mut t.events, 15, |e| {
        matches!(
            e,
            SessionEvent::Lifecycle(Lifecycle::Exited {
                reason: ExitReason::AppExited
            })
        )
    })
    .await;
}

#[tokio::test]
async fn an_undeclared_privilege_throws_security_error_and_raises_a_diagnostic() {
    let mut t = start("tizen-apis");
    t.session.wait_until_loaded().await.unwrap();
    let thrown = t
        .session
        .evaluate(
            "(function () { try { tizen.application.getAppsInfo(function () {}); return 'no error'; } \
               catch (e) { return [e.name, e instanceof tizen.WebAPIException]; } })()",
        )
        .await
        .unwrap();
    assert_eq!(thrown, json!(["SecurityError", true]));
    expect_event(&mut t.events, 10, |e| {
        matches!(e, SessionEvent::Diagnostic(d)
            if d.code == "privilege-not-declared" && d.message.contains(APP_INFO_PRIVILEGE))
    })
    .await;
    t.session.stop().await;
}

#[tokio::test]
async fn lenient_mode_allows_the_call_but_still_raises_the_diagnostic() {
    let mut t = start_with("tizen-apis", |c| c.lenient = true);
    t.session.wait_until_loaded().await.unwrap();
    let apps = eval_callback(
        &t.session,
        "tizen.application.getAppsInfo(function (apps) { done(apps.length); }, \
           function (e) { done(e.name); });",
    )
    .await;
    assert!(apps.as_u64().is_some_and(|n| n >= 1), "{apps}");
    expect_event(&mut t.events, 10, |e| {
        matches!(e, SessionEvent::Diagnostic(d)
            if d.code == "privilege-not-declared" && d.message.contains(APP_INFO_PRIVILEGE))
    })
    .await;
    t.session.stop().await;
}

#[tokio::test]
async fn a_declared_privilege_allows_the_call() {
    let mut t = start("tizen-apis-privileged");
    t.session.wait_until_loaded().await.unwrap();
    let ids = eval_callback(
        &t.session,
        "tizen.application.getAppsInfo(function (apps) { \
           done(apps.map(function (a) { return a.id; })); }, function (e) { done(e.name); });",
    )
    .await;
    assert_eq!(ids, json!(["Fixture005.Privileged"]));
    t.session.stop().await;
    let mut diagnostics = 0;
    while let Ok(Some(event)) =
        tokio::time::timeout(std::time::Duration::from_millis(100), t.events.next()).await
    {
        if matches!(event, SessionEvent::Diagnostic(_)) {
            diagnostics += 1;
        }
    }
    assert_eq!(diagnostics, 0);
}

#[tokio::test]
async fn the_bridge_refuses_connections_without_the_session_token() {
    let t = start("tizen-apis");
    t.session.wait_until_loaded().await.unwrap();
    let url = t
        .session
        .evaluate("window.__TIZEN_SIM__.bridgeUrl")
        .await
        .unwrap();
    let url = url.as_str().unwrap();
    assert!(url.contains("token="), "{url}");
    let forged = format!(
        "{}forged",
        &url[..url.find("token=").unwrap() + "token=".len()]
    );
    assert!(tokio_tungstenite::connect_async(forged).await.is_err());
    t.session.stop().await;
}
