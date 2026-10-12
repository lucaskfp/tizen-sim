//! The Shim bootstrap and the first Tizen API Shims, exercised through a
//! Session on a real Engine.

mod support;

use serde_json::json;
use support::{EXIT_SECS, eval_callback, eventually_equals, expect_event, start, start_with};
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
    // Major.Minor.Patch (systeminfo_capability_keys.html).
    assert_eq!(version, format!("{}.0", support::profile().version));

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

    // Both LANGUAGE_REGION (systeminfo.html#SystemInfoLocale::country).
    let locale = eval_callback(
        &t.session,
        "tizen.systeminfo.getPropertyValue('LOCALE', function (l) { done([l.language, l.country]); });",
    )
    .await;
    assert_eq!(locale, json!(["en_US", "en_US"]));
    t.session.stop().await;
}

/// systeminfo.html#SystemInfo::getPropertyValue: a string that isn't a
/// SystemInfoPropertyId throws TypeMismatchError; a valid property the TV
/// doesn't support goes to the error callback as NotSupportedError.
#[tokio::test]
async fn systeminfo_validates_property_ids() {
    let t = start("tizen-apis");
    t.session.wait_until_loaded().await.unwrap();
    let thrown = t
        .session
        .evaluate(
            "(function () { \
               function attempt(f) { try { f(); return 'no error'; } catch (e) { return e.name; } } \
               var s = tizen.systeminfo; \
               return [ \
                 attempt(function () { s.getPropertyValue('NOT_A_PROPERTY', function () {}); }), \
                 attempt(function () { s.getPropertyValueArray('NOT_A_PROPERTY', function () {}); }), \
                 attempt(function () { s.getCount('NOT_A_PROPERTY'); }), \
                 s.getCount('BATTERY'), s.getCount('MEMORY') \
               ]; })()",
        )
        .await
        .unwrap();
    assert_eq!(
        thrown,
        json!([
            "TypeMismatchError",
            "TypeMismatchError",
            "TypeMismatchError",
            0,
            1
        ])
    );
    let unsupported = eval_callback(
        &t.session,
        "tizen.systeminfo.getPropertyValue('BATTERY', function () { done('success'); }, \
           function (e) { done(e.name); });",
    )
    .await;
    assert_eq!(unsupported, "NotSupportedError");
    t.session.stop().await;
}

/// The TV properties and methods systeminfo.html documents beyond the basics
/// (TD guides/device/system-information.md lists the properties).
#[tokio::test]
async fn systeminfo_reports_memory_network_and_panel_and_more() {
    let t = start("tizen-apis");
    t.session.wait_until_loaded().await.unwrap();
    let values = eval_callback(
        &t.session,
        "var s = tizen.systeminfo, out = {}; \
         s.getPropertyValue('MEMORY', function (m) { out.memory = m.status; \
           s.getPropertyValue('NETWORK', function (n) { out.network = n.networkType; \
             s.getPropertyValueArray('DISPLAY', function (d) { out.displays = d.length; \
               done(out); }); }); });",
    )
    .await;
    assert_eq!(
        values,
        json!({ "memory": "NORMAL", "network": "ETHERNET", "displays": 1 })
    );
    // PANEL is a property since Tizen 5.5 (systeminfo.html#SystemInfoPanel);
    // before, the string isn't a SystemInfoPropertyId.
    let panel = eval_callback(
        &t.session,
        "try { tizen.systeminfo.getPropertyValue('PANEL', function (p) { \
           done([p.panelWidth > 0, p.panelHeight > 0]); }); } catch (e) { done(e.name); }",
    )
    .await;
    if support::has_api("SystemInfoPropertyId.PANEL") {
        assert_eq!(panel, json!([true, true]));
    } else {
        assert_eq!(panel, json!("TypeMismatchError"));
    }

    // Listeners return ids and may never fire; removing an unknown id is a
    // no-op. PANEL can't be watched (systeminfo.html#SystemInfoPanel).
    let listeners = eval_callback(
        &t.session,
        "var s = tizen.systeminfo; \
         var a = s.addPropertyValueChangeListener('BUILD', function () {}); \
         var b = s.addPropertyValueChangeListener('MEMORY', function () {}); \
         s.removePropertyValueChangeListener(a); s.removePropertyValueChangeListener(12345); \
         done([typeof a, a !== b]);",
    )
    .await;
    assert_eq!(listeners, json!(["number", true]));
    if support::has_api("SystemInfoPropertyId.PANEL") {
        let panel = eval_callback(
            &t.session,
            "tizen.systeminfo.addPropertyValueChangeListener('PANEL', function () {}, null, \
               function (e) { done(e.name); });",
        )
        .await;
        assert_eq!(panel, json!("NotSupportedError"));
    }

    // Since Tizen 8.0 (systeminfo.html#SystemInfo::getDeviceUptime).
    if support::has_api("tizen.systeminfo.getDeviceUptime") {
        let uptime = t
            .session
            .evaluate("tizen.systeminfo.getDeviceUptime()")
            .await
            .unwrap();
        assert!(uptime.as_u64().is_some(), "{uptime}");
    }
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

/// `visibilitychange` also fires when the App exits (TV
/// guides/fundamentals/multitasking.html; TD guides/concepts/web-runtime.md).
#[tokio::test]
async fn exit_fires_visibilitychange_and_ends_the_session() {
    let mut t = start("tizen-apis");
    t.session.wait_until_loaded().await.unwrap();
    t.session
        .evaluate(
            "document.addEventListener('visibilitychange', function () { \
               console.log('visibilitychange hidden=' + document.hidden); }); \
             tizen.application.getCurrentApplication().exit(); 0",
        )
        .await
        .unwrap();
    expect_event(
        &mut t.events,
        15,
        |e| matches!(e, SessionEvent::Console(m) if m.text == "visibilitychange hidden=true"),
    )
    .await;
    expect_event(&mut t.events, EXIT_SECS, |e| {
        matches!(
            e,
            SessionEvent::Lifecycle(Lifecycle::Exited {
                reason: ExitReason::AppExited
            })
        )
    })
    .await;
}

/// An App still busy when it exits gets its `visibilitychange` too: the
/// Session waits for the page to answer instead of closing the Engine.
#[tokio::test]
async fn exit_fires_visibilitychange_in_an_app_still_busy() {
    let mut t = start("tizen-apis");
    t.session.wait_until_loaded().await.unwrap();
    t.session
        .evaluate(
            "document.addEventListener('visibilitychange', function () {                console.log('visibilitychange hidden=' + document.hidden); });              setTimeout(function () {                tizen.application.getCurrentApplication().exit();                var until = Date.now() + 2000; while (Date.now() < until) {}              }, 0); 0",
        )
        .await
        .unwrap();
    expect_event(
        &mut t.events,
        15,
        |e| matches!(e, SessionEvent::Console(m) if m.text == "visibilitychange hidden=true"),
    )
    .await;
    expect_event(&mut t.events, EXIT_SECS, |e| {
        matches!(
            e,
            SessionEvent::Lifecycle(Lifecycle::Exited {
                reason: ExitReason::AppExited
            })
        )
    })
    .await;
}

/// A page-side counter that ticks every 50ms, to tell whether JavaScript runs.
const START_TICKING: &str = "window.ticks = 0; setInterval(function () { window.ticks++; }, 50); \
     window.visibilityLog = []; document.addEventListener('visibilitychange', function () { \
       window.visibilityLog.push(document.hidden); }); 0";

async fn ticks(session: &tizen_sim_core::session::Session) -> u64 {
    session
        .evaluate("window.ticks")
        .await
        .unwrap()
        .as_u64()
        .unwrap()
}

/// application.html#Application::hide backgrounds the App: `document.hidden`
/// becomes true, `visibilitychange` fires and, without background support,
/// JavaScript pauses (TV guides/fundamentals/multitasking.html). Bringing it
/// back fires `visibilitychange` again and resumes it.
#[tokio::test]
async fn hide_backgrounds_and_pauses_the_app_until_it_is_resumed() {
    let mut t = start("tizen-apis");
    t.session.wait_until_loaded().await.unwrap();
    t.session.evaluate(START_TICKING).await.unwrap();
    t.session
        .evaluate("tizen.application.getCurrentApplication().hide(); 0")
        .await
        .unwrap();
    expect_event(&mut t.events, 10, |e| {
        matches!(e, SessionEvent::Lifecycle(Lifecycle::Hidden))
    })
    .await;
    assert_eq!(
        t.session
            .evaluate("[document.hidden, window.visibilityLog]")
            .await
            .unwrap(),
        json!([true, [true]])
    );
    let paused_at = ticks(&t.session).await;
    // Longer than the once-a-second timers of a merely hidden page.
    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
    assert_eq!(
        ticks(&t.session).await,
        paused_at,
        "JavaScript kept running"
    );

    t.session.resume().await.unwrap();
    eventually_equals(
        &t.session,
        "[document.hidden, window.visibilityLog]",
        json!([false, [true, false]]),
    )
    .await;
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    assert!(
        ticks(&t.session).await > paused_at,
        "JavaScript did not resume"
    );
    t.session.stop().await;
}

/// `<tizen:setting background-support="enable"/>` with a background category
/// keeps the App running while hidden (TD reference/config-xml.md).
#[tokio::test]
async fn background_support_keeps_a_hidden_app_running() {
    let t = start("background-support");
    t.session.wait_until_loaded().await.unwrap();
    t.session.evaluate(START_TICKING).await.unwrap();
    t.session
        .evaluate("tizen.application.getCurrentApplication().hide(); 0")
        .await
        .unwrap();
    eventually_equals(&t.session, "document.hidden", json!(true)).await;
    let hidden_at = ticks(&t.session).await;
    // Hidden pages' timers are throttled to once a second.
    tokio::time::sleep(std::time::Duration::from_millis(2500)).await;
    assert!(ticks(&t.session).await > hidden_at, "JavaScript was paused");
    t.session.stop().await;
}

/// application.html#Application::getRequestedAppControl never returns null;
/// a plain launch carries the default operation.
#[tokio::test]
async fn the_requested_app_control_describes_a_plain_launch() {
    let t = start("tizen-apis");
    t.session.wait_until_loaded().await.unwrap();
    let control = t
        .session
        .evaluate(
            "(function () { var r = tizen.application.getCurrentApplication().getRequestedAppControl(); \
               return [r.appControl.operation, r.appControl.data, typeof r.callerAppId, \
                       r.callerAppId.length > 0]; })()",
        )
        .await
        .unwrap();
    assert_eq!(
        control,
        json!([
            "http://tizen.org/appcontrol/operation/default",
            [],
            "string",
            true
        ])
    );
    t.session.stop().await;
}

/// application.html#ApplicationManager::getAppContext.
#[tokio::test]
async fn get_app_context_describes_the_running_app() {
    let t = start("tizen-apis");
    t.session.wait_until_loaded().await.unwrap();
    let contexts = t
        .session
        .evaluate(
            "(function () { var a = tizen.application, mine = a.getAppContext(), unknown; \
               try { a.getAppContext('no-such-context'); unknown = 'no error'; } \
               catch (e) { unknown = e.name; } \
               return [mine.appId, mine.id === a.getCurrentApplication().contextId, \
                       a.getAppContext(mine.id).appId, a.getAppContext(null).appId, unknown]; })()",
        )
        .await
        .unwrap();
    assert_eq!(
        contexts,
        json!([
            "Fixture004.TizenApis",
            true,
            "Fixture004.TizenApis",
            "Fixture004.TizenApis",
            "NotFoundError"
        ])
    );
    t.session.stop().await;
}

/// `application.info` guards only ApplicationInformation.size, not
/// getAppsInfo or getAppInfo (application.html#ApplicationManager::getAppsInfo,
/// #ApplicationInformation::size).
#[tokio::test]
async fn reading_app_size_without_its_privilege_throws_security_error() {
    let mut t = start("tizen-apis");
    t.session.wait_until_loaded().await.unwrap();
    let apps = eval_callback(
        &t.session,
        "tizen.application.getAppsInfo(function (apps) { \
           done(apps.map(function (a) { return a.id; })); }, function (e) { done(e.name); });",
    )
    .await;
    assert_eq!(apps, json!(["Fixture004.TizenApis"]));
    let thrown = t
        .session
        .evaluate(
            "(function () { var info = tizen.application.getAppInfo(); \
               try { return 'size ' + info.size; } \
               catch (e) { return [info.id, e.name, e instanceof tizen.WebAPIException]; } })()",
        )
        .await
        .unwrap();
    assert_eq!(
        thrown,
        json!(["Fixture004.TizenApis", "SecurityError", true])
    );
    expect_event(&mut t.events, 10, |e| {
        matches!(e, SessionEvent::Diagnostic(d)
            if d.code == "privilege-not-declared" && d.message.contains(APP_INFO_PRIVILEGE))
    })
    .await;
    t.session.stop().await;
}

#[tokio::test]
async fn lenient_mode_allows_the_read_but_still_raises_the_diagnostic() {
    let mut t = start_with("tizen-apis", |c| c.lenient = true);
    t.session.wait_until_loaded().await.unwrap();
    let size = t
        .session
        .evaluate("typeof tizen.application.getAppInfo().size")
        .await
        .unwrap();
    assert_eq!(size, "number");
    expect_event(&mut t.events, 10, |e| {
        matches!(e, SessionEvent::Diagnostic(d)
            if d.code == "privilege-not-declared" && d.message.contains(APP_INFO_PRIVILEGE))
    })
    .await;
    t.session.stop().await;
}

#[tokio::test]
async fn a_declared_privilege_allows_the_read() {
    let mut t = start("tizen-apis-privileged");
    t.session.wait_until_loaded().await.unwrap();
    let size = t
        .session
        .evaluate("typeof tizen.application.getAppInfo().size")
        .await
        .unwrap();
    assert_eq!(size, "number");
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

/// tizen.html#WebAPIException and TD guides/concepts/error-handling.md:
/// DOM-named errors carry their legacy DOMException code, Tizen-only names 0,
/// and the code constants are exposed.
#[tokio::test]
async fn web_api_exceptions_carry_the_documented_codes_and_constants() {
    let t = start("tizen-apis");
    t.session.wait_until_loaded().await.unwrap();
    let codes = t
        .session
        .evaluate(
            "['NotFoundError', 'NotSupportedError', 'InvalidStateError', 'TypeMismatchError', \
              'SecurityError', 'NetworkError', 'AbortError', 'QuotaExceededError', 'TimeoutError', \
              'InvalidValuesError', 'IOError', 'ServiceNotAvailableError', 'UnknownError', \
              'VerificationError'].map(function (name) { \
               return new tizen.WebAPIException(name, '').code; })",
        )
        .await
        .unwrap();
    assert_eq!(
        codes,
        json!([8, 9, 11, 17, 18, 19, 20, 22, 23, 0, 0, 0, 0, 0])
    );

    let constants = t
        .session
        .evaluate(
            "(function () { var E = tizen.WebAPIException, thrown; \
               try { tizen.systeminfo.getPropertyValue('BUILD', 'not a function'); } \
               catch (e) { thrown = e; } \
               return [E.INDEX_SIZE_ERR, E.SECURITY_ERR, E.DATA_CLONE_ERR, \
                       E.prototype.TYPE_MISMATCH_ERR, thrown.TYPE_MISMATCH_ERR === thrown.code]; })()",
        )
        .await
        .unwrap();
    assert_eq!(constants, json!([1, 18, 25, 17, true]));
    t.session.stop().await;
}
