//! The TV screen: resolution, black transparency, the checkerboard and
//! DevTools, at the Session seam.

mod support;

use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::time::Duration;
use support::{expect_event, pixel, start, start_with};
use tizen_sim_core::screen::AppResolution;
use tizen_sim_core::session::{Lifecycle, Session, SessionEvent};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

const SIZE: &str = "[innerWidth, innerHeight, screen.width, screen.height]";

/// The Engine's page debugging address, from the Session's events.
async fn debugger_url(session: &Session) -> String {
    let mut events = session.events();
    match expect_event(&mut events, 5, |e| {
        matches!(e, SessionEvent::Lifecycle(Lifecycle::EngineReady { .. }))
    })
    .await
    {
        SessionEvent::Lifecycle(Lifecycle::EngineReady { debugger_url, .. }) => debugger_url,
        _ => unreachable!(),
    }
}

/// One CDP call on a connection of the test's own, with an optional
/// `Origin` header.
async fn cdp_call(ws_url: &str, origin: Option<&str>, method: &str, params: Value) -> Value {
    try_cdp_call(ws_url, origin, method, params)
        .await
        .unwrap_or_else(|error| panic!("{method}: {error}"))
}

/// Like `cdp_call`, failing with why the connection or the call failed.
async fn try_cdp_call(
    ws_url: &str,
    origin: Option<&str>,
    method: &str,
    params: Value,
) -> Result<Value, String> {
    let mut request = ws_url.into_client_request().unwrap();
    if let Some(origin) = origin {
        request
            .headers_mut()
            .insert("Origin", origin.parse().unwrap());
    }
    let (mut socket, _) = tokio_tungstenite::connect_async(request)
        .await
        .map_err(|e| e.to_string())?;
    let call = json!({ "id": 1, "method": method, "params": params });
    socket.send(Message::text(call.to_string())).await.unwrap();
    while let Some(message) = socket.next().await {
        if let Message::Text(text) = message.unwrap() {
            let reply: Value = serde_json::from_str(&text).unwrap();
            if reply["id"] == 1 {
                return match reply.get("error") {
                    Some(error) => Err(error.to_string()),
                    None => Ok(reply["result"].clone()),
                };
            }
        }
    }
    Err("no reply".into())
}

fn png_size(png: &[u8]) -> (u32, u32) {
    let reader = png::Decoder::new(std::io::Cursor::new(png))
        .read_info()
        .unwrap();
    (reader.info().width, reader.info().height)
}

const RED: [u8; 3] = [255, 0, 0];
const BLACK: [u8; 3] = [0, 0, 0];

#[tokio::test]
async fn the_app_is_laid_out_at_1920x1080_whatever_the_window_size() {
    let t = start("screen");
    t.session.wait_until_loaded().await.unwrap();
    assert_eq!(
        t.session.evaluate(SIZE).await.unwrap(),
        json!([1920, 1080, 1920, 1080])
    );

    let ws = debugger_url(&t.session).await;
    // M47 takes one debugging client per page, the Session's.
    let window = try_cdp_call(&ws, None, "Browser.getWindowForTarget", target(&ws)).await;
    if let Ok(window) = window {
        cdp_call(
            &ws,
            None,
            "Browser.setWindowBounds",
            json!({ "windowId": window["windowId"], "bounds": { "width": 800, "height": 600 } }),
        )
        .await;
    } else {
        // Engines without the Browser domain (M47) let an app window resize itself.
        t.session.evaluate("resizeTo(800, 600); 0").await.unwrap();
    }
    // Give the Session time to notice and rescale.
    tokio::time::sleep(Duration::from_secs(2)).await;
    assert_eq!(
        t.session.evaluate(SIZE).await.unwrap(),
        json!([1920, 1080, 1920, 1080])
    );
    let shot = t.session.screenshot().await.unwrap();
    assert_eq!(png_size(&shot), (1920, 1080));
    // The App's own corners, wherever the Engine drew it in the window (#57).
    // (Not black: the test's debugging client leaving undoes that on M130.)
    assert_eq!(pixel(&shot, 50, 50), RED);
    assert_ne!(pixel(&shot, 150, 50), RED);
    assert_ne!(pixel(&shot, 1910, 1070), RED);
    t.session.stop().await;
}

/// `Browser.getWindowForTarget`'s parameters for the page at `ws_url`.
fn target(ws_url: &str) -> Value {
    json!({ "targetId": ws_url.rsplit('/').next().unwrap() })
}

#[tokio::test]
async fn a_resized_window_is_snapped_to_the_apps_aspect() {
    let t = start("screen");
    t.session.wait_until_loaded().await.unwrap();
    let ws = debugger_url(&t.session).await;
    let Ok(window) = try_cdp_call(&ws, None, "Browser.getWindowForTarget", target(&ws)).await
    else {
        // Engines without the Browser domain (M47, M56) can't be resized;
        // M47 centers the App instead (see the test above).
        return;
    };
    let window_id = &window["windowId"];
    cdp_call(
        &ws,
        None,
        "Browser.setWindowBounds",
        json!({ "windowId": window_id, "bounds": { "width": 800, "height": 600 } }),
    )
    .await;
    // Too tall for the App's 16:9 by both sides changing: the window keeps
    // its width and loses the leftover height once it settles.
    let mut bounds = Value::Null;
    for _ in 0..20 {
        tokio::time::sleep(Duration::from_millis(250)).await;
        let window = cdp_call(
            &ws,
            None,
            "Browser.getWindowBounds",
            json!({ "windowId": window_id }),
        )
        .await;
        bounds = window["bounds"].clone();
        if bounds["height"] != 600 {
            break;
        }
    }
    assert_eq!(bounds["width"], 800, "{bounds}");
    assert!(bounds["height"].as_f64().unwrap() < 600.0, "{bounds}");
    t.session.stop().await;
}

#[tokio::test]
async fn the_fhd_mode_is_1280x720() {
    let t = start_with("screen", |c| c.resolution = AppResolution::Fhd);
    t.session.wait_until_loaded().await.unwrap();
    assert_eq!(
        t.session.evaluate(SIZE).await.unwrap(),
        json!([1280, 720, 1280, 720])
    );
    assert_eq!(
        png_size(&t.session.screenshot().await.unwrap()),
        (1280, 720)
    );
    t.session.stop().await;
}

#[tokio::test]
async fn transparent_areas_are_black_or_a_checkerboard_with_the_toggle() {
    let t = start("screen");
    t.session.wait_until_loaded().await.unwrap();
    let shot = t.session.screenshot().await.unwrap();
    assert_eq!(pixel(&shot, 50, 50), RED);
    assert_eq!(pixel(&shot, 1000, 500), BLACK);

    t.session.set_checkerboard(true).await.unwrap();
    let shot = t.session.screenshot().await.unwrap();
    assert_eq!(pixel(&shot, 50, 50), RED, "App content covers the checks");
    // Two neighbouring squares, light and dark.
    let (a, b) = (pixel(&shot, 1000, 500), pixel(&shot, 1016, 500));
    assert_ne!(a, b, "no checkerboard");
    for colour in [a, b] {
        assert!(colour[0] > 150 && colour[0] == colour[1], "{colour:?}");
    }

    // An opaque root background covers the whole page, so nothing is
    // transparent any more.
    t.session
        .evaluate("document.documentElement.style.background = '#00f'; 0")
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(600)).await;
    let shot = t.session.screenshot().await.unwrap();
    assert_eq!(pixel(&shot, 1000, 500), [0, 0, 255]);
    assert_eq!(pixel(&shot, 1016, 500), [0, 0, 255]);

    // A semi-transparent one lets the checks show through.
    t.session
        .evaluate("document.documentElement.style.background = 'rgba(0, 0, 255, 0.5)'; 0")
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(600)).await;
    let shot = t.session.screenshot().await.unwrap();
    let (a, b) = (pixel(&shot, 1000, 500), pixel(&shot, 1016, 500));
    assert_ne!(
        a, b,
        "no checkerboard under the half-transparent background"
    );
    assert!(a[2] > a[0] && b[2] > b[0], "{a:?} {b:?} aren't tinted blue");

    // It stays on across a reload, and off means black again.
    t.session
        .evaluate("window.beforeReload = true; location.reload(); 0")
        .await
        .unwrap();
    // The new document, with the checkerboard back.
    support::eventually_equals(
        &t.session,
        "!window.beforeReload && !!document.querySelector('[data-tizen-sim=checkerboard]')",
        json!(true),
    )
    .await;
    t.session.set_checkerboard(false).await.unwrap();
    let shot = t.session.screenshot().await.unwrap();
    assert_eq!(pixel(&shot, 1000, 500), BLACK);
    assert_eq!(pixel(&shot, 1016, 500), BLACK);
    t.session.stop().await;
}

#[tokio::test]
async fn the_devtools_address_is_the_engines_own_working_inspector() {
    let t = start("screen");
    t.session.wait_until_loaded().await.unwrap();
    let url = t.session.devtools_url().unwrap();

    let response = tokio::time::timeout(Duration::from_secs(10), reqwest::get(&url))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(response.status(), 200);
    let page = response.text().await.unwrap();
    assert!(page.to_lowercase().contains("<html"), "{page}");

    // The inspector connects from its own origin to the address it's given.
    let origin = url.split("/devtools/").next().unwrap();
    let ws = format!("ws://{}", url.split("ws=").nth(1).unwrap());
    let result = cdp_call(
        &ws,
        Some(origin),
        "Runtime.evaluate",
        json!({ "expression": "document.title", "returnByValue": true }),
    )
    .await;
    assert_eq!(result["result"]["value"], "Screen fixture");
    t.session.stop().await;
}
