//! Remote Keys: keyboard mapping, registration enforcement and delivery,
//! exercised through a Session on a real Engine.

mod support;

use serde_json::json;
use std::time::Duration;
use support::{eventually_equals, expect_event, start, start_with};
use tizen_sim_core::remote_keys::PhysicalKey;
use tizen_sim_core::session::{KeyAction, KeyOutcome, SessionEvent};

const BACKSPACE: PhysicalKey = PhysicalKey(8);
const F5: PhysicalKey = PhysicalKey(116);
const DIGIT_0: PhysicalKey = PhysicalKey(48);

async fn tap(session: &tizen_sim_core::session::Session, key: PhysicalKey) {
    session.physical_key(key, KeyAction::Down).await.unwrap();
    session.physical_key(key, KeyAction::Up).await.unwrap();
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
async fn a_long_press_of_back_delivers_exit() {
    let t = start("remote-keys");
    t.session.wait_until_loaded().await.unwrap();
    t.session
        .evaluate("tizen.tvinputdevice.registerKey('Exit'); 0")
        .await
        .unwrap();
    t.session
        .physical_key(BACKSPACE, KeyAction::Down)
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(1300)).await;
    t.session
        .physical_key(BACKSPACE, KeyAction::Up)
        .await
        .unwrap();
    eventually_equals(&t.session, "window.keyLog", json!([10182])).await;
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
