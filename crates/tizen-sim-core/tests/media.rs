//! Whether each Engine plays what a TV plays: H.264/AAC in MP4, listed in
//! every year's TV video specifications (TV specifications/media-specifications/
//! <year>-tv-video-specifications.html, 2017 to 2026). An Engine that can't
//! raises the `media-format-unsupported` Diagnostic when the App tries.
//!
//! Run once per Engine with `TIZEN_SIM_TEST_PROFILE`, as the nightly job does.

mod support;

use serde_json::{Value, json};
use support::{eval_callback, eventually_pixel, expect_event, profile, start};
use tizen_sim_core::session::{Diagnostic, Events, Session, SessionEvent};

/// The single colour of `media/clip.mp4`, a grey so that how the Engine
/// converts H.264's colours can't change it, and the test pattern's first bar.
const CLIP_GREY: [u8; 3] = [0x60, 0x60, 0x60];
const PATTERN_GREY: [u8; 3] = [0xc0, 0xc0, 0xc0];

const MP4_H264_AAC: &str = r#"video/mp4; codecs="avc1.42E01E, mp4a.40.2""#;

/// Chrome for Testing (M113 on) is built with H.264 and AAC; the Chromium
/// snapshots older Engines come from are not (measured on every Engine,
/// #105).
fn engine_plays_h264_aac() -> bool {
    profile().milestone >= 113
}

async fn eval(session: &Session, expression: &str) -> Value {
    session.evaluate(expression).await.unwrap()
}

/// What the Engine says about `type`: `canPlayType` and
/// `MediaSource.isTypeSupported`, printed so each Engine's run records it.
async fn support_for(session: &Session, mime: &str) -> (String, bool) {
    let answer = eval(
        session,
        &format!(
            "[document.createElement('video').canPlayType({type}), MediaSource.isTypeSupported({type})]",
            type = json!(mime)
        ),
    )
    .await;
    println!("Tizen {}: {mime}: {answer}", profile().version);
    (
        answer[0].as_str().unwrap().to_string(),
        answer[1].as_bool().unwrap(),
    )
}

/// Waits for the `media-format-unsupported` Diagnostic mentioning `about`.
async fn expect_unsupported_format(events: &mut Events, about: &str) -> Diagnostic {
    match expect_event(events, 15, |e| {
        matches!(e, SessionEvent::Diagnostic(d)
            if d.code == "media-format-unsupported" && d.message.contains(about))
    })
    .await
    {
        SessionEvent::Diagnostic(d) => d,
        _ => unreachable!(),
    }
}

/// Fails the test if the App was told about a format, waiting a moment for
/// Diagnostics still on their way.
async fn assert_no_unsupported_format(events: &mut Events) {
    let wait = async {
        while let Some(event) = events.next().await {
            if let SessionEvent::Diagnostic(d) = &event {
                assert_ne!(d.code, "media-format-unsupported", "{d:?}");
            }
        }
    };
    let _ = tokio::time::timeout(std::time::Duration::from_secs(2), wait).await;
}

#[tokio::test]
async fn a_video_element_plays_h264_aac_mp4_or_the_app_is_told_the_engine_cant() {
    let mut t = start("avplay");
    t.session.wait_until_loaded().await.unwrap();
    let s = &t.session;
    let (can_play, _) = support_for(s, MP4_H264_AAC).await;
    let played = eval_callback(
        s,
        "var v = document.createElement('video'); v.muted = true; \
         v.addEventListener('canplay', function () { done(v.videoWidth + 'x' + v.videoHeight); }); \
         v.addEventListener('error', function () { done('error ' + v.error.code); }); \
         v.src = mediaUrl('clip.mp4'); document.body.appendChild(v);",
    )
    .await;
    println!("Tizen {}: <video> clip.mp4: {played}", profile().version);

    if engine_plays_h264_aac() {
        assert_eq!(can_play, "probably");
        assert_eq!(played, "320x180");
        assert_no_unsupported_format(&mut t.events).await;
    } else {
        assert_eq!(can_play, "");
        // MEDIA_ERR_SRC_NOT_SUPPORTED.
        assert_eq!(played, "error 4");
        let asked = expect_unsupported_format(&mut t.events, "canPlayType").await;
        assert!(asked.message.contains("H.264/AAC in MP4"), "{asked:?}");
        let failed = expect_unsupported_format(&mut t.events, "clip.mp4").await;
        assert!(failed.message.contains("H.264 and AAC"), "{failed:?}");
    }
}

#[tokio::test]
async fn avplay_plays_h264_aac_mp4_or_the_app_is_told_the_engine_cant() {
    let mut t = start("avplay");
    t.session.wait_until_loaded().await.unwrap();
    let s = &t.session;
    let prepared = eval_callback(
        s,
        "webapis.avplay.open(mediaUrl('clip.mp4'));          webapis.avplay.setDisplayRect(0, 0, 1920, 1080);          webapis.avplay.prepareAsync(function () { webapis.avplay.play();            done(webapis.avplay.getState()); }, function (e) { done(e.name); });",
    )
    .await;
    assert_eq!(prepared, "PLAYING");

    if engine_plays_h264_aac() {
        eventually_pixel(s, 100, 300, CLIP_GREY).await;
        // The clip's own length, not the simulated one.
        assert_eq!(eval(s, "webapis.avplay.getDuration() < 10000").await, true);
        assert_no_unsupported_format(&mut t.events).await;
    } else {
        eventually_pixel(s, 100, 300, PATTERN_GREY).await;
        let diagnostic = expect_unsupported_format(&mut t.events, "clip.mp4").await;
        assert!(
            diagnostic.message.contains("H.264 and AAC"),
            "{diagnostic:?}"
        );
    }
    println!(
        "Tizen {}: AVPlay clip.mp4 plays: {}",
        profile().version,
        engine_plays_h264_aac()
    );
}

#[tokio::test]
async fn media_source_takes_h264_aac_mp4_or_the_app_is_told_the_engine_cant() {
    let mut t = start("avplay");
    t.session.wait_until_loaded().await.unwrap();
    let s = &t.session;
    let (_, supported) = support_for(s, MP4_H264_AAC).await;
    let added = eval_callback(
        s,
        &format!(
            "var source = new MediaSource();              source.addEventListener('sourceopen', function () {{                try {{ source.addSourceBuffer({type}); done('added'); }}                catch (e) {{ done(e.name); }} }});              var v = document.createElement('video');              v.src = URL.createObjectURL(source); document.body.appendChild(v);",
            type = json!(MP4_H264_AAC)
        ),
    )
    .await;

    if engine_plays_h264_aac() {
        assert!(supported);
        assert_eq!(added, "added");
        assert_no_unsupported_format(&mut t.events).await;
    } else {
        assert!(!supported);
        assert_eq!(added, "NotSupportedError");
        expect_unsupported_format(&mut t.events, "isTypeSupported").await;
        let failed = expect_unsupported_format(&mut t.events, "addSourceBuffer").await;
        assert!(failed.message.contains("H.264/AAC in MP4"), "{failed:?}");
    }
}
