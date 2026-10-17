//! Primary seam: the `gamepad` metadata's launch check, on a real Engine
//! (TV guides/user-interaction/gamepad.html, "Checking for Specific Gamepads").

mod support;

use support::{STARTUP_SECS, expect_event, start, start_with};
use tizen_sim_core::session::{
    ExitReason, KeyOutcome, Lifecycle, SessionError, SessionEvent, SessionStatus,
};

fn is_gamepad_required(event: &SessionEvent) -> bool {
    matches!(
        event,
        SessionEvent::Lifecycle(Lifecycle::GamepadRequired { .. })
    )
}

fn is_app_loaded(event: &SessionEvent) -> bool {
    matches!(event, SessionEvent::Lifecycle(Lifecycle::AppLoaded { .. }))
}

fn exited_because(event: &SessionEvent, expected: ExitReason) -> bool {
    matches!(event, SessionEvent::Lifecycle(Lifecycle::Exited { reason }) if *reason == expected)
}

/// The events up to and including the first that matches `until`.
async fn events_until(
    events: &mut tizen_sim_core::session::Events,
    until: impl Fn(&SessionEvent) -> bool,
) -> Vec<SessionEvent> {
    let collect = async {
        let mut seen = Vec::new();
        while let Some(event) = events.next().await {
            let done = until(&event);
            seen.push(event);
            if done {
                return seen;
            }
        }
        panic!("event stream ended early: {seen:#?}");
    };
    tokio::time::timeout(std::time::Duration::from_secs(STARTUP_SECS), collect)
        .await
        .expect("timed out waiting for the event")
}

#[tokio::test]
async fn without_a_required_gamepad_the_platform_asks_before_launching_and_play_launches() {
    let mut t = start("gamepad");
    let event = expect_event(&mut t.events, STARTUP_SECS, is_gamepad_required).await;
    assert!(matches!(
        event,
        SessionEvent::Lifecycle(Lifecycle::GamepadRequired { models }) if models == ["dualshock4"]
    ));
    assert_eq!(t.session.status(), SessionStatus::Starting);

    // Play has the focus, as in the guide's figure.
    let outcome = t.session.press_key("Enter").await.unwrap();
    assert_eq!(outcome, KeyOutcome::Delivered);
    expect_event(&mut t.events, 30, is_app_loaded).await;
    let title = t.session.evaluate("document.title").await.unwrap();
    assert_eq!(title, "Gamepad fixture");
    // The popup belongs to the platform, so it isn't in the App's history:
    // the App has the history it has when launched without the popup.
    let history = t.session.evaluate("history.length").await.unwrap();
    let without_popup = start_with("gamepad", |config| {
        config.gamepads = vec!["dualshock4".into()]
    });
    without_popup.session.wait_until_loaded().await.unwrap();
    let expected = without_popup
        .session
        .evaluate("history.length")
        .await
        .unwrap();
    without_popup.session.stop().await;
    assert_eq!(history, expected);
    t.session.stop().await;
}

#[tokio::test]
async fn cancel_in_the_popup_does_not_launch_the_app() {
    let mut t = start("gamepad");
    expect_event(&mut t.events, STARTUP_SECS, is_gamepad_required).await;
    t.session.press_key("ArrowRight").await.unwrap();
    t.session.press_key("Enter").await.unwrap();

    let seen = events_until(&mut t.events, |e| {
        matches!(e, SessionEvent::Lifecycle(Lifecycle::Exited { .. }))
    })
    .await;
    assert!(exited_because(
        seen.last().unwrap(),
        ExitReason::LaunchCanceled
    ));
    assert!(!seen.iter().any(is_app_loaded), "{seen:#?}");
    assert_eq!(t.session.status(), SessionStatus::Exited);
}

#[tokio::test]
async fn connect_now_in_the_popup_does_not_launch_the_app() {
    let mut t = start("gamepad");
    expect_event(&mut t.events, STARTUP_SECS, is_gamepad_required).await;
    t.session.press_key("ArrowRight").await.unwrap();
    t.session.press_key("ArrowRight").await.unwrap();
    t.session.press_key("Enter").await.unwrap();

    let seen = events_until(&mut t.events, |e| {
        matches!(e, SessionEvent::Lifecycle(Lifecycle::Exited { .. }))
    })
    .await;
    assert!(exited_because(
        seen.last().unwrap(),
        ExitReason::LaunchCanceled
    ));
    assert!(!seen.iter().any(is_app_loaded), "{seen:#?}");
}

#[tokio::test]
async fn a_connected_required_gamepad_launches_the_app_without_asking() {
    let mut t = start_with("gamepad", |config| {
        config.gamepads = vec!["usbgamepad".into(), "dualshock4".into()];
    });
    let seen = events_until(&mut t.events, is_app_loaded).await;
    assert!(!seen.iter().any(is_gamepad_required), "{seen:#?}");
    t.session.stop().await;
}

#[tokio::test]
async fn an_app_without_the_metadata_launches_without_asking() {
    let mut t = start("hello");
    let seen = events_until(&mut t.events, is_app_loaded).await;
    assert!(!seen.iter().any(is_gamepad_required), "{seen:#?}");
    t.session.stop().await;
}

#[tokio::test]
async fn stopping_during_the_popup_leaves_nothing_to_press_keys_on() {
    let mut t = start("gamepad");
    expect_event(&mut t.events, STARTUP_SECS, is_gamepad_required).await;
    t.session.stop().await;
    expect_event(&mut t.events, 30, |e| {
        exited_because(e, ExitReason::Stopped)
    })
    .await;
    let pressed = t.session.press_key("Enter").await;
    assert!(
        matches!(pressed, Err(SessionError::NotLoaded)),
        "{pressed:?}"
    );
}

#[tokio::test]
async fn an_engine_that_dies_during_the_popup_ends_the_session_as_crashed() {
    let mut t = start("gamepad");
    expect_event(&mut t.events, STARTUP_SECS, is_gamepad_required).await;
    let pid = t
        .session
        .engine_pid()
        .expect("the popup's Engine has a process");

    support::kill_process(pid);

    let seen = events_until(&mut t.events, |e| {
        matches!(e, SessionEvent::Lifecycle(Lifecycle::Exited { .. }))
    })
    .await;
    assert!(
        exited_because(seen.last().unwrap(), ExitReason::EngineCrashed),
        "{seen:#?}"
    );
}
