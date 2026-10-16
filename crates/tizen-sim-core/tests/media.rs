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
/// snapshots older Engines come from are not. Measured on every Windows
/// Engine (#105); on Linux it follows from Chromium's `proprietary_codecs`
/// build flag, on only for Chrome-branded and Chrome for Testing builds.
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

/// A failed MP4 stream's Diagnostic: Engines before M69 play no MP4 at all,
/// later snapshots MP4 without H.264 or AAC.
fn assert_blames_mp4_or_h264_and_aac(diagnostic: &Diagnostic) {
    let message = &diagnostic.message;
    assert!(
        message.contains("has no MP4") || (message.contains("H.264") && message.contains("AAC")),
        "{diagnostic:?}"
    );
    assert!(
        message.contains(&format!("Tizen {} TVs play", profile().version)),
        "{diagnostic:?}"
    );
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
        assert_blames_mp4_or_h264_and_aac(&failed);
    }
}

#[tokio::test]
async fn avplay_plays_h264_aac_mp4_or_the_app_is_told_the_engine_cant() {
    let mut t = start("avplay");
    t.session.wait_until_loaded().await.unwrap();
    let s = &t.session;
    let prepared = eval_callback(
        s,
        "webapis.avplay.open(mediaUrl('clip.mp4')); \
         webapis.avplay.setDisplayRect(0, 0, 1920, 1080); \
         webapis.avplay.prepareAsync(function () { webapis.avplay.play(); \
           done(webapis.avplay.getState()); }, function (e) { done(e.name); });",
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
        assert_blames_mp4_or_h264_and_aac(&diagnostic);
    }
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
            "var source = new MediaSource(); \
             source.addEventListener('sourceopen', function () {{ \
               try {{ source.addSourceBuffer({type}); done('added'); }} \
               catch (e) {{ done(e.name); }} }}); \
             var v = document.createElement('video'); \
             v.src = URL.createObjectURL(source); document.body.appendChild(v);",
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

/// Asks the Engine `canPlayType(type)`, as an App deciding what to play.
async fn can_play_type(session: &Session, mime: &str) -> String {
    let (answer, _) = support_for(session, mime).await;
    answer
}

/// Formats every Tizen TV lists that no Engine plays, the newest included
/// (measured, #105): Dolby audio and MPEG-TS.
#[tokio::test]
async fn formats_tvs_play_and_no_engine_does_raise_the_diagnostic() {
    let mut t = start("avplay");
    t.session.wait_until_loaded().await.unwrap();
    let s = &t.session;
    let version = &profile().version;
    for (mime, format) in [
        (r#"audio/mp4; codecs="ac-3""#, "AC-3 in MP4"),
        (r#"audio/mp4; codecs="ec-3""#, "E-AC-3 in MP4"),
        (
            r#"video/mp2t; codecs="avc1.42E01E, mp4a.40.2""#,
            "H.264/AAC in MPEG-TS",
        ),
    ] {
        assert_eq!(can_play_type(s, mime).await, "", "{mime}");
        let diagnostic = expect_unsupported_format(&mut t.events, format).await;
        assert!(
            diagnostic
                .message
                .contains(&format!("Tizen {version} TVs play {format}")),
            "{diagnostic:?}"
        );
    }
}

/// HEVC: every TV year lists it; whether an Engine plays it depends on the
/// Engine (and, for Chrome for Testing, on the computer's video decoder).
#[tokio::test]
async fn hevc_raises_the_diagnostic_only_where_the_engine_lacks_it() {
    let mut t = start("avplay");
    t.session.wait_until_loaded().await.unwrap();
    let answer = can_play_type(&t.session, r#"video/mp4; codecs="hev1.1.6.L93.B0""#).await;
    if answer.is_empty() {
        let diagnostic = expect_unsupported_format(&mut t.events, "HEVC in MP4").await;
        // Engines before M69 play no MP4 at all.
        let message = &diagnostic.message;
        assert!(
            message.contains("has no HEVC") || message.contains("has no MP4"),
            "{diagnostic:?}"
        );
    } else {
        assert_no_unsupported_format(&mut t.events).await;
    }
}

/// The 2024 tables list WMA on every model group but 24TV_BASIC4; the 2025
/// and 2026 ones on none (media specifications, Music table and its notes).
#[tokio::test]
async fn a_format_only_some_models_play_says_so_and_one_none_play_raises_nothing() {
    let mut t = start("avplay");
    t.session.wait_until_loaded().await.unwrap();
    let s = &t.session;
    // Dolby Vision: no TV year lists it.
    can_play_type(s, r#"video/mp4; codecs="dvh1.05.06""#).await;
    assert_eq!(can_play_type(s, "audio/x-ms-wma").await, "");
    let version = profile().version.as_str();
    match version {
        "9.0" | "10.0" => assert_no_unsupported_format(&mut t.events).await,
        _ => {
            let diagnostic = expect_unsupported_format(&mut t.events, "WMA").await;
            let expected = if version == "8.0" {
                "Only some Tizen 8.0 TV models (10 of 11 model groups) play WMA"
            } else {
                &format!("Tizen {version} TVs play WMA")
            };
            assert!(diagnostic.message.contains(expected), "{diagnostic:?}");
            assert!(!diagnostic.message.contains("dvh1"), "{diagnostic:?}");
        }
    }
}

/// AVPlay plays HLS, MPEG-DASH and Smooth Streaming on a TV
/// (guides/multimedia/adaptive-streaming.html); no Engine has a player for
/// them, so the Video Plane shows the test pattern.
#[tokio::test]
async fn avplay_tells_the_app_the_engine_cant_play_hls() {
    let mut t = start("avplay");
    t.session.wait_until_loaded().await.unwrap();
    eval(
        &t.session,
        "webapis.avplay.open(mediaUrl('live/stream.m3u8'));          webapis.avplay.setDisplayRect(0, 0, 1920, 1080); webapis.avplay.prepareAsync(); 0",
    )
    .await;
    let diagnostic = expect_unsupported_format(&mut t.events, "stream.m3u8").await;
    assert!(
        diagnostic.message.contains("On a TV AVPlay plays HLS"),
        "{diagnostic:?}"
    );
}
