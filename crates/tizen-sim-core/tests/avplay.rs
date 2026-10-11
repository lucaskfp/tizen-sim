//! `webapis.avplay` and its Video Plane (ADR-0004), exercised through a
//! Session on a real Engine.

mod support;

use serde_json::{Value, json};
use std::time::Duration;
use support::{eval_callback, eventually_equals, expect_event, pixel, start, start_with};
use tizen_sim_core::screen::AppResolution;
use tizen_sim_core::session::{Session, SessionEvent};

const DRMPLAY_PRIVILEGE: &str = "http://developer.samsung.com/privilege/drmplay";

/// The test pattern's first bar, and what's behind the page.
const PATTERN_GREY: [u8; 3] = [0xc0, 0xc0, 0xc0];
const BLACK: [u8; 3] = [0, 0, 0];
const RED: [u8; 3] = [255, 0, 0];
/// The single colour of `media/clip.webm`.
const CLIP_BLUE: [u8; 3] = [0x20, 0x80, 0xe0];

/// Runs `body` and returns the name of what it throws, or `"no error"`.
fn attempt(body: &str) -> String {
    format!(
        "(function () {{ try {{ {body}; return 'no error'; }} catch (e) {{ return e.name; }} }})()"
    )
}

async fn eval(session: &Session, expression: &str) -> Value {
    session.evaluate(expression).await.unwrap()
}

async fn player_state(session: &Session) -> Value {
    eval(session, "webapis.avplay.getState()").await
}

/// Opens one of the fixture's media files, recording every playback callback.
async fn open(session: &Session, media: &str) {
    eval(
        session,
        &format!("recordEvents(); webapis.avplay.open(mediaUrl('{media}')); 0"),
    )
    .await;
}

/// The names of the playback callbacks called so far, in order.
async fn event_names(session: &Session) -> Vec<String> {
    serde_json::from_value(eval(session, "avEvents.map(function (e) { return e[0]; })").await)
        .unwrap()
}

fn near(actual: [u8; 3], expected: [u8; 3]) -> bool {
    actual
        .iter()
        .zip(expected)
        .all(|(a, e)| a.abs_diff(e) <= 24)
}

/// Waits until the screen shows about `expected` at `(x, y)`.
async fn eventually_pixel(session: &Session, x: u32, y: u32, expected: [u8; 3]) {
    let mut last = [0; 3];
    for _ in 0..50 {
        last = pixel(&session.screenshot().await.unwrap(), x, y);
        if near(last, expected) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    panic!("the screen at ({x}, {y}) never became {expected:?}; last {last:?}");
}

/// using-avplay.html: NONE → IDLE (`open`) → READY (`prepare`) →
/// PLAYING/PAUSED; `stop()` returns to IDLE and `close()` to NONE. The
/// allowed states per method are avplay-api.html's.
#[tokio::test]
async fn avplay_goes_through_the_documented_states_and_events() {
    let t = start("avplay");
    t.session.wait_until_loaded().await.unwrap();
    let s = &t.session;
    assert_eq!(player_state(s).await, "NONE");
    let refused = eval(
        s,
        &format!(
            "[{}, {}, {}, {}]",
            attempt("webapis.avplay.play()"),
            attempt("webapis.avplay.prepare()"),
            attempt("webapis.avplay.pause()"),
            attempt("webapis.avplay.open(42)"),
        ),
    )
    .await;
    assert_eq!(
        refused,
        json!([
            "InvalidStateError",
            "InvalidStateError",
            "InvalidStateError",
            "TypeMismatchError"
        ])
    );

    open(s, "not-a-video.webm").await;
    assert_eq!(player_state(s).await, "IDLE");
    let prepared = eval_callback(
        s,
        "webapis.avplay.prepareAsync(function () { done(webapis.avplay.getState()); }, \
           function (e) { done(e.name); });",
    )
    .await;
    assert_eq!(prepared, "READY");
    let names = event_names(s).await;
    assert_eq!(names.first().map(String::as_str), Some("onbufferingstart"));
    assert_eq!(
        names.last().map(String::as_str),
        Some("onbufferingcomplete")
    );
    assert!(
        names[1..names.len() - 1]
            .iter()
            .all(|name| name == "onbufferingprogress")
    );
    assert_eq!(
        eval(
            s,
            "avEvents.filter(function (e) { return e[0] === 'onbufferingprogress'; }).pop()[1]"
        )
        .await,
        100
    );

    eval(s, "avEvents = []; webapis.avplay.play(); 0").await;
    assert_eq!(player_state(s).await, "PLAYING");
    eventually_equals(
        s,
        "avEvents.some(function (e) { return e[0] === 'oncurrentplaytime' && e[1] >= 1000; })",
        json!(true),
    )
    .await;

    eval(s, "webapis.avplay.pause(); 0").await;
    assert_eq!(player_state(s).await, "PAUSED");
    let paused_at = eval(s, "webapis.avplay.getCurrentTime()").await;
    tokio::time::sleep(Duration::from_millis(600)).await;
    assert_eq!(eval(s, "webapis.avplay.getCurrentTime()").await, paused_at);
    assert_eq!(
        eval(s, &attempt("webapis.avplay.open(mediaUrl('clip.webm'))")).await,
        "InvalidStateError"
    );

    let seeked = eval_callback(
        s,
        "webapis.avplay.seekTo(5000, function () { done(webapis.avplay.getCurrentTime()); }, \
           function (e) { done(e.name); });",
    )
    .await;
    assert_eq!(seeked, 5000);

    eval(s, "webapis.avplay.stop(); 0").await;
    assert_eq!(player_state(s).await, "IDLE");
    eval(s, "webapis.avplay.close(); 0").await;
    assert_eq!(player_state(s).await, "NONE");
    t.session.stop().await;
}

/// A stream the Engine can decode plays for real: its own duration, its
/// frames in the Video Plane, and `onstreamcompleted` at its end.
#[tokio::test]
async fn decodable_streams_play_in_the_video_plane() {
    let t = start("avplay");
    t.session.wait_until_loaded().await.unwrap();
    let s = &t.session;
    open(s, "clip.webm").await;
    eval(
        s,
        "webapis.avplay.setDisplayRect(0, 0, 1920, 1080); webapis.avplay.prepare(); \
         webapis.avplay.play(); 0",
    )
    .await;
    eventually_pixel(s, 960, 300, CLIP_BLUE).await;
    // The App's opaque box covers the video.
    assert_eq!(pixel(&s.screenshot().await.unwrap(), 1800, 950), RED);

    let duration = eval(s, "webapis.avplay.getDuration()").await;
    assert!(
        duration.as_u64().is_some_and(|ms| ms.abs_diff(3000) < 200),
        "duration {duration}"
    );
    eventually_equals(
        s,
        "avEvents.some(function (e) { return e[0] === 'onstreamcompleted'; })",
        json!(true),
    )
    .await;

    // A seek after the end plays on from there.
    eval(s, "avEvents = []; webapis.avplay.seekTo(0); 0").await;
    eventually_equals(
        s,
        "avEvents.filter(function (e) { return e[0] === 'oncurrentplaytime'; }).length >= 2",
        json!(true),
    )
    .await;
    t.session.stop().await;
}

/// The Video Plane sits where `setDisplayRect` puts it, in 1920x1080
/// coordinates (avplay-api.html), behind the page: opaque App content covers
/// it. A stream the Engine can't decode shows the test pattern.
#[tokio::test]
async fn the_video_plane_sits_at_the_display_rect_behind_the_page() {
    let t = start("avplay");
    t.session.wait_until_loaded().await.unwrap();
    let s = &t.session;
    open(s, "not-a-video.webm").await;
    eval(
        s,
        "webapis.avplay.setDisplayRect(960, 540, 960, 540); webapis.avplay.prepare(); \
         webapis.avplay.play(); 0",
    )
    .await;
    eventually_pixel(s, 1000, 600, PATTERN_GREY).await;
    let shot = s.screenshot().await.unwrap();
    assert_eq!(pixel(&shot, 1800, 950), RED, "App content covers the plane");
    assert_eq!(pixel(&shot, 900, 600), BLACK, "left of the display area");
    assert_eq!(pixel(&shot, 1000, 500), BLACK, "above the display area");

    // stop() takes the video away.
    eval(s, "webapis.avplay.stop(); 0").await;
    eventually_pixel(s, 1000, 600, BLACK).await;
    t.session.stop().await;
}

/// On an FHD TV the App's screen is 1280x720, but setDisplayRect still takes
/// 1920x1080 coordinates (using-avplay.html: "the TV screen resolution is
/// always treated as 1920x1080 px").
#[tokio::test]
async fn the_display_rect_is_in_1920x1080_coordinates_on_fhd_tvs() {
    let t = start_with("avplay", |c| c.resolution = AppResolution::Fhd);
    t.session.wait_until_loaded().await.unwrap();
    let s = &t.session;
    open(s, "not-a-video.webm").await;
    eval(
        s,
        "webapis.avplay.setDisplayRect(960, 540, 960, 540); webapis.avplay.prepare();          webapis.avplay.play(); 0",
    )
    .await;
    // 960 TV pixels are 640 App px.
    eventually_pixel(s, 660, 380, PATTERN_GREY).await;
    assert_eq!(pixel(&s.screenshot().await.unwrap(), 620, 380), BLACK);
    t.session.stop().await;
}

/// The canvas background paints beneath every element, so an opaque `html`
/// background, or a `body` one that propagates to the canvas, hides the Video
/// Plane on a TV (ADR-0004).
#[tokio::test]
async fn an_opaque_canvas_background_hides_the_video_plane() {
    let mut t = start("avplay");
    t.session.wait_until_loaded().await.unwrap();
    let s = &t.session;
    open(s, "not-a-video.webm").await;
    eval(
        s,
        "webapis.avplay.setDisplayRect(0, 0, 1920, 1080); webapis.avplay.prepare(); \
         webapis.avplay.play(); 0",
    )
    .await;
    eventually_pixel(s, 100, 100, PATTERN_GREY).await;

    eval(s, "document.body.style.background = 'rgb(0, 0, 128)'; 0").await;
    let event = expect_event(
        &mut t.events,
        10,
        |e| matches!(e, SessionEvent::Diagnostic(d) if d.code == "video-plane-hidden"),
    )
    .await;
    let SessionEvent::Diagnostic(diagnostic) = event else {
        unreachable!()
    };
    assert!(
        diagnostic.message.contains("<body>") && diagnostic.message.contains("rgb(0, 0, 128)"),
        "{}",
        diagnostic.message
    );
    eventually_pixel(&t.session, 100, 100, [0, 0, 128]).await;

    eval(
        &t.session,
        "document.documentElement.style.background = 'rgb(0, 128, 0)'; 0",
    )
    .await;
    expect_event(&mut t.events, 10, |e| {
        matches!(e, SessionEvent::Diagnostic(d) if d.code == "video-plane-hidden"
            && d.message.contains("<html>") && d.message.contains("rgb(0, 128, 0)"))
    })
    .await;

    // A semi-transparent background lets the video through.
    eval(
        &t.session,
        "document.documentElement.style.background = 'rgba(0, 128, 0, 0.5)'; 0",
    )
    .await;
    eventually_pixel(&t.session, 100, 100, [0x60, 0xa0, 0x60]).await;
    t.session.stop().await;
}

/// avplay-api.html: `setDrm` and `getUID` need the `drmplay` privilege.
#[tokio::test]
async fn drm_methods_need_the_drmplay_privilege() {
    let mut t = start("avplay");
    t.session.wait_until_loaded().await.unwrap();
    open(&t.session, "clip.webm").await;
    let thrown = eval(
        &t.session,
        &format!(
            "[{}, {}]",
            attempt("webapis.avplay.setDrm('PLAYREADY', 'SetProperties', '{}')"),
            attempt("webapis.avplay.getUID('PLAYREADY')"),
        ),
    )
    .await;
    assert_eq!(thrown, json!(["SecurityError", "SecurityError"]));
    expect_event(&mut t.events, 10, |e| {
        matches!(e, SessionEvent::Diagnostic(d)
            if d.code == "privilege-not-declared" && d.message.contains(DRMPLAY_PRIVILEGE))
    })
    .await;
    t.session.stop().await;
}
