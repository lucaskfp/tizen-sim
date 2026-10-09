//! Remote Keys: keyboard mapping, registration enforcement and delivery,
//! exercised through a Session on a real Engine.

mod support;

use serde_json::json;
use std::time::Duration;
use support::{eventually_equals, expect_event, start, start_with};
use tizen_sim_core::remote_keys::PhysicalKey;
use tizen_sim_core::session::{ExitReason, KeyAction, KeyOutcome, Lifecycle, SessionEvent};

const BACKSPACE: PhysicalKey = PhysicalKey(8);
const F5: PhysicalKey = PhysicalKey(116);
const DIGIT_0: PhysicalKey = PhysicalKey(48);

async fn tap(session: &tizen_sim_core::session::Session, key: PhysicalKey) {
    session.physical_key(key, KeyAction::Down).await.unwrap();
    session.physical_key(key, KeyAction::Up).await.unwrap();
}

/// Holds a key long enough to auto-repeat twice.
async fn hold(session: &tizen_sim_core::session::Session, key: PhysicalKey) {
    for action in [
        KeyAction::Down,
        KeyAction::Repeat,
        KeyAction::Repeat,
        KeyAction::Up,
    ] {
        session.physical_key(key, action).await.unwrap();
    }
}

fn dropped(event: &SessionEvent, key: &str) -> bool {
    matches!(event, SessionEvent::RemoteKey { key: k, outcome: KeyOutcome::Dropped { reason } }
        if k == key && reason.contains("not registered"))
}

#[tokio::test]
async fn backspace_in_the_engine_window_delivers_back() {
    let t = start("remote-keys");
    t.session.wait_until_loaded().await.unwrap();
    tap(&t.session, BACKSPACE).await;
    eventually_equals(&t.session, "window.keyLog", json!([10009])).await;
    t.session.stop().await;
}

#[tokio::test]
async fn a_registrable_key_is_dropped_until_registered() {
    let mut t = start("remote-keys");
    t.session.wait_until_loaded().await.unwrap();

    t.session.press_key("ColorF0Red").await.unwrap();
    expect_event(&mut t.events, 10, |e| dropped(e, "ColorF0Red")).await;
    assert_eq!(
        t.session.evaluate("window.keyLog").await.unwrap(),
        json!([])
    );

    t.session
        .evaluate("tizen.tvinputdevice.registerKey('ColorF0Red'); 0")
        .await
        .unwrap();
    t.session.press_key("ColorF0Red").await.unwrap();
    eventually_equals(&t.session, "window.keyLog", json!([403])).await;
    expect_event(&mut t.events, 10, |e| {
        matches!(e, SessionEvent::RemoteKey { key, outcome: KeyOutcome::Delivered } if key == "ColorF0Red")
    })
    .await;
    t.session.stop().await;
}

#[tokio::test]
async fn mandatory_keys_are_always_delivered_and_cannot_be_registered() {
    let t = start("remote-keys");
    t.session.wait_until_loaded().await.unwrap();
    for key in [
        "ArrowLeft",
        "ArrowUp",
        "ArrowRight",
        "ArrowDown",
        "Enter",
        "Back",
    ] {
        t.session.press_key(key).await.unwrap();
    }
    eventually_equals(
        &t.session,
        "window.keyLog",
        json!([37, 38, 39, 40, 13, 10009]),
    )
    .await;
    let error = t
        .session
        .evaluate(
            "(function () { try { tizen.tvinputdevice.registerKey('Enter'); return 'registered'; } \
               catch (e) { return e.name; } })()",
        )
        .await
        .unwrap();
    assert_eq!(error, "InvalidValuesError");
    t.session.stop().await;
}

#[tokio::test]
async fn supported_keys_are_the_registrable_keys_of_the_version_profile() {
    let t = start("remote-keys");
    t.session.wait_until_loaded().await.unwrap();
    let keys = t
        .session
        .evaluate(
            "tizen.tvinputdevice.getSupportedKeys().map(function (k) { return k.name + ':' + k.code; })",
        )
        .await
        .unwrap();
    let keys: Vec<String> = serde_json::from_value(keys).unwrap();
    for expected in [
        "ColorF0Red:403",
        "Exit:10182",
        "MediaPlayPause:10252",
        "0:48",
        "ChannelUp:427",
    ] {
        assert!(
            keys.contains(&expected.to_string()),
            "{expected} missing from {keys:?}"
        );
    }
    for mandatory in ["Enter:13", "Back:10009", "ArrowLeft:37"] {
        assert!(
            !keys.contains(&mandatory.to_string()),
            "{mandatory} listed as supported"
        );
    }
    t.session.stop().await;
}

#[tokio::test]
/// "Long-pressing the Return/Exit key forces the application to terminate"
/// (Samsung's terminating-applications guide): the App gets no key at all.
async fn a_long_press_of_back_terminates_the_app() {
    let mut t = start("remote-keys");
    t.session.wait_until_loaded().await.unwrap();
    t.session
        .physical_key(BACKSPACE, KeyAction::Down)
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(1300)).await;
    expect_event(&mut t.events, 10, terminated_by_remote).await;
}

fn terminated_by_remote(event: &SessionEvent) -> bool {
    matches!(
        event,
        SessionEvent::Lifecycle(Lifecycle::Exited {
            reason: ExitReason::TerminatedByRemote
        })
    )
}

/// The Exit key closes the App and shows the TV channel screen (Samsung's
/// development checklist, CO-US-04; CO-GE-07: "When the return or exit key is
/// clicked, the application is terminated"). Registering it changes nothing.
#[tokio::test]
async fn the_exit_key_terminates_the_app_even_when_registered() {
    let mut t = start("remote-keys");
    t.session.wait_until_loaded().await.unwrap();
    t.session
        .evaluate("tizen.tvinputdevice.registerKey('Exit'); 0")
        .await
        .unwrap();
    t.session.press_key("Exit").await.unwrap();
    // Reported as dropped: the App never gets it.
    expect_event(&mut t.events, 10, |e| {
        matches!(e, SessionEvent::RemoteKey { key, outcome: KeyOutcome::Dropped { .. } } if key == "Exit")
    })
    .await;
    expect_event(&mut t.events, 10, terminated_by_remote).await;
}

#[tokio::test]
async fn registering_the_exit_key_raises_a_diagnostic_saying_it_never_arrives() {
    let mut t = start("remote-keys");
    t.session.wait_until_loaded().await.unwrap();
    t.session
        .evaluate("tizen.tvinputdevice.registerKey('Exit'); 0")
        .await
        .unwrap();
    expect_event(&mut t.events, 10, |e| {
        matches!(e, SessionEvent::Diagnostic(d)
            if d.code == "exit-key-registered" && d.message.contains("never receive"))
    })
    .await;
    t.session.stop().await;
}

/// tvinputdevice.html#TVInputDeviceManager::getKey: null only for an
/// unsupported key, InvalidValuesError for an invalid name such as "".
#[tokio::test]
async fn get_key_rejects_invalid_names_and_returns_null_for_unsupported_ones() {
    let t = start("remote-keys");
    t.session.wait_until_loaded().await.unwrap();
    let result = t
        .session
        .evaluate(
            "(function () {                var d = tizen.tvinputdevice, invalid;                try { d.getKey(''); invalid = 'no error'; } catch (e) { invalid = e.name; }                var red = d.getKey('ColorF0Red'), enter = d.getKey('Enter');                return [invalid, d.getKey('NotAKey'), red.name + ':' + red.code,                        enter && enter.name + ':' + enter.code]; })()",
        )
        .await
        .unwrap();
    assert_eq!(
        result,
        json!(["InvalidValuesError", null, "ColorF0Red:403", "Enter:13"])
    );
    t.session.stop().await;
}

/// tvinputdevice.html#TVInputDeviceManager::registerKeyBatch: arguments of the
/// wrong type throw TypeMismatchError synchronously.
#[tokio::test]
async fn batch_methods_reject_arguments_of_the_wrong_type_synchronously() {
    let t = start("remote-keys");
    t.session.wait_until_loaded().await.unwrap();
    let result = t
        .session
        .evaluate(
            "(function () {                function attempt(f) { try { f(); return 'no error'; } catch (e) { return e.name; } }                var d = tizen.tvinputdevice;                return [                  attempt(function () { d.registerKeyBatch('ColorF0Red'); }),                  attempt(function () { d.registerKeyBatch(['ColorF0Red'], 'not a function'); }),                  attempt(function () { d.unregisterKeyBatch(['ColorF0Red'], null, {}); }),                  attempt(function () { d.registerKeyBatch(['ColorF0Red'], null, null); })                ]; })()",
        )
        .await
        .unwrap();
    assert_eq!(
        result,
        json!([
            "TypeMismatchError",
            "TypeMismatchError",
            "TypeMismatchError",
            "no error"
        ])
    );
    t.session.stop().await;
}

/// An unregistered key produces neither keydown nor keyup (TD
/// guides/text-input/input-device.md), auto-repeats included.
#[tokio::test]
async fn an_unregistered_key_sends_no_repeat_and_no_keyup() {
    let t = start("remote-keys");
    t.session.wait_until_loaded().await.unwrap();
    hold(&t.session, DIGIT_0).await;
    t.session
        .evaluate("tizen.tvinputdevice.registerKey('1'); 0")
        .await
        .unwrap();
    hold(&t.session, PhysicalKey(49)).await;
    eventually_equals(&t.session, "window.keyUpLog", json!([49])).await;
    assert_eq!(
        t.session.evaluate("window.keyLog").await.unwrap(),
        json!([49, 49, 49])
    );
    t.session.stop().await;
}

#[tokio::test]
async fn an_unregistered_physical_digit_is_dropped() {
    let mut t = start("remote-keys");
    t.session.wait_until_loaded().await.unwrap();
    tap(&t.session, DIGIT_0).await;
    expect_event(&mut t.events, 10, |e| dropped(e, "0")).await;
    assert_eq!(
        t.session.evaluate("window.keyLog").await.unwrap(),
        json!([])
    );

    t.session
        .evaluate("tizen.tvinputdevice.registerKey('0'); 0")
        .await
        .unwrap();
    tap(&t.session, DIGIT_0).await;
    eventually_equals(&t.session, "window.keyLog", json!([48])).await;
    t.session.stop().await;
}

#[tokio::test]
async fn lenient_mode_delivers_unregistered_keys_with_a_diagnostic() {
    let mut t = start_with("remote-keys", |c| c.lenient = true);
    t.session.wait_until_loaded().await.unwrap();
    t.session.press_key("ColorF1Green").await.unwrap();
    eventually_equals(&t.session, "window.keyLog", json!([404])).await;
    expect_event(&mut t.events, 10, |e| {
        matches!(e, SessionEvent::Diagnostic(d)
            if d.code == "key-not-registered" && d.message.contains("ColorF1Green"))
    })
    .await;
    t.session.stop().await;
}

#[tokio::test]
async fn the_keyboard_mapping_is_configurable() {
    let t = start_with("remote-keys", |c| {
        c.key_map
            .insert(PhysicalKey::parse("F5").unwrap(), "MediaPlayPause")
            .unwrap();
    });
    t.session.wait_until_loaded().await.unwrap();
    t.session
        .evaluate("tizen.tvinputdevice.registerKeyBatch(['MediaPlayPause', 'ColorF3Blue']); 0")
        .await
        .unwrap();
    tap(&t.session, F5).await;
    // A mapped keyboard key travels through the Bridge, so a key pressed
    // through the Session right after it could overtake it.
    eventually_equals(&t.session, "window.keyLog", json!([10252])).await;
    t.session.press_key("ColorF3Blue").await.unwrap();
    eventually_equals(&t.session, "window.keyLog", json!([10252, 406])).await;
    t.session.stop().await;
}

#[tokio::test]
async fn an_unknown_remote_key_is_an_error() {
    let t = start("remote-keys");
    t.session.wait_until_loaded().await.unwrap();
    let err = t.session.press_key("NotAKey").await.unwrap_err();
    assert!(err.to_string().contains("NotAKey"), "{err}");
    t.session.stop().await;
}

#[tokio::test]
async fn lenient_mode_reports_unregistered_physical_keys_too() {
    let mut t = start_with("remote-keys", |c| c.lenient = true);
    t.session.wait_until_loaded().await.unwrap();
    tap(&t.session, DIGIT_0).await;
    eventually_equals(&t.session, "window.keyLog", json!([48])).await;
    expect_event(&mut t.events, 10, |e| {
        matches!(e, SessionEvent::Diagnostic(d)
            if d.code == "key-not-registered" && d.message.contains("0 is not registered"))
    })
    .await;
    t.session.stop().await;
}

#[tokio::test]
async fn long_presses_of_rewind_and_fast_forward_skip_tracks() {
    let t = start_with("remote-keys", |c| {
        c.key_map
            .insert(PhysicalKey::parse("F6").unwrap(), "MediaRewind")
            .unwrap();
        c.key_map
            .insert(PhysicalKey::parse("F7").unwrap(), "MediaFastForward")
            .unwrap();
    });
    t.session.wait_until_loaded().await.unwrap();
    t.session
        .evaluate(
            "tizen.tvinputdevice.registerKeyBatch(['MediaRewind', 'MediaTrackPrevious', \
               'MediaFastForward', 'MediaTrackNext']); 0",
        )
        .await
        .unwrap();
    let (f6, f7) = (PhysicalKey(117), PhysicalKey(118));
    tap(&t.session, f6).await;
    for key in [f6, f7] {
        t.session.physical_key(key, KeyAction::Down).await.unwrap();
        tokio::time::sleep(Duration::from_millis(1300)).await;
        t.session.physical_key(key, KeyAction::Up).await.unwrap();
    }
    eventually_equals(&t.session, "window.keyLog", json!([412, 10232, 10233])).await;
    t.session.stop().await;
}

#[tokio::test]
async fn the_full_documented_key_set_is_supported() {
    let t = start("remote-keys");
    t.session.wait_until_loaded().await.unwrap();
    let keys = t
        .session
        .evaluate(
            "tizen.tvinputdevice.getSupportedKeys().map(function (k) { return k.name + ':' + k.code; })",
        )
        .await
        .unwrap();
    let keys: Vec<String> = serde_json::from_value(keys).unwrap();
    for expected in [
        "Minus:189",
        "ChannelList:10073",
        "PreviousChannel:10190",
        "Menu:18",
        "Tools:10135",
        "Info:457",
        "Source:10072",
        "Caption:10221",
        "E-Manual:10146",
        "3D:10199",
        "Extra:10253",
        "PictureSize:10140",
        "Soccer:10228",
        "Teletext:10200",
        "MTS:10195",
        "Search:10225",
        "Guide:458",
    ] {
        assert!(keys.contains(&expected.to_string()), "{expected} missing");
    }
    t.session.stop().await;
}

#[tokio::test]
async fn typing_in_a_text_field_is_not_mistaken_for_remote_keys() {
    let t = start("remote-keys");
    t.session.wait_until_loaded().await.unwrap();
    t.session
        .evaluate(
            "var input = document.createElement('input'); document.body.appendChild(input); \
             input.focus(); 0",
        )
        .await
        .unwrap();
    // Minus (189) is a Registrable Key, but here it's just a hyphen being typed.
    tap(&t.session, PhysicalKey(189)).await;
    eventually_equals(&t.session, "window.keyLog", json!([189])).await;
    t.session.stop().await;
}
