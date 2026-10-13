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

/// The Engine window, through the `Browser` domain where the Engine has it,
/// or else through the page: an app window may resize itself (M56, #65).
struct WindowControl<'a> {
    session: &'a Session,
    ws: &'a str,
    window_id: Option<Value>,
}

impl WindowControl<'_> {
    async fn resize(&self, width: u32, height: u32) {
        match &self.window_id {
            Some(id) => {
                let bounds = json!({ "width": width, "height": height });
                let params = json!({ "windowId": id, "bounds": bounds });
                cdp_call(self.ws, None, "Browser.setWindowBounds", params).await;
            }
            None => {
                let resize = format!("resizeTo({width}, {height}); 0");
                self.session.evaluate(&resize).await.unwrap();
            }
        }
    }

    /// Moves the window's top left corner to `left`, `top`.
    async fn move_to(&self, left: i64, top: i64) {
        match &self.window_id {
            Some(id) => {
                let bounds = json!({ "left": left, "top": top });
                let params = json!({ "windowId": id, "bounds": bounds });
                cdp_call(self.ws, None, "Browser.setWindowBounds", params).await;
            }
            None => {
                let move_to = format!("moveTo({left}, {top}); 0");
                self.session.evaluate(&move_to).await.unwrap();
            }
        }
    }

    /// `[left, top, width, height]` of the display's work area, from
    /// `Screen`'s own getters: in a page of its own where the Session's page
    /// reports the TV's screen in their place (M63+).
    async fn work_area(&self) -> Vec<i64> {
        const AREA: &str =
            "['availLeft', 'availTop', 'availWidth', 'availHeight'].map(function (n) {
          return Object.getOwnPropertyDescriptor(Screen.prototype, n).get.call(screen);
        })";
        let area = match &self.window_id {
            Some(_) => {
                let params = json!({ "url": "about:blank" });
                let page = cdp_call(self.ws, None, "Target.createTarget", params).await;
                let id = page["targetId"].as_str().unwrap();
                let page_ws = format!("{}/{id}", self.ws.rsplit_once('/').unwrap().0);
                let params = json!({ "expression": AREA, "returnByValue": true });
                let area = cdp_call(&page_ws, None, "Runtime.evaluate", params).await;
                let params = json!({ "targetId": id });
                cdp_call(self.ws, None, "Target.closeTarget", params).await;
                area["result"]["value"].clone()
            }
            None => self.session.evaluate(AREA).await.unwrap(),
        };
        serde_json::from_value(area).unwrap()
    }

    /// `[left, top]` of the whole window.
    async fn position(&self) -> Value {
        match &self.window_id {
            Some(id) => {
                let params = json!({ "windowId": id });
                let window = cdp_call(self.ws, None, "Browser.getWindowBounds", params).await;
                json!([window["bounds"]["left"], window["bounds"]["top"]])
            }
            None => self.session.evaluate("[screenX, screenY]").await.unwrap(),
        }
    }

    /// `[width, height]` of the whole window.
    async fn size(&self) -> Value {
        match &self.window_id {
            Some(id) => {
                let params = json!({ "windowId": id });
                let window = cdp_call(self.ws, None, "Browser.getWindowBounds", params).await;
                json!([window["bounds"]["width"], window["bounds"]["height"]])
            }
            None => self
                .session
                .evaluate("[outerWidth, outerHeight]")
                .await
                .unwrap(),
        }
    }
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
    // M47 takes one debugging client per page, the Session's.
    let window = try_cdp_call(&ws, None, "Browser.getWindowForTarget", target(&ws)).await;
    if window.is_err() && support::profile().version == "3.0" {
        // M47 centers the App instead (see the test above).
        return;
    }
    // Too tall for the App's 16:9 by both sides changing: the window keeps
    // its width and loses the leftover height once it settles.
    let window = WindowControl {
        session: &t.session,
        ws: &ws,
        window_id: window.ok().map(|window| window["windowId"].clone()),
    };
    window.resize(800, 600).await;
    let mut size = Value::Null;
    for _ in 0..20 {
        tokio::time::sleep(Duration::from_millis(250)).await;
        size = window.size().await;
        if size[1] != 600 {
            break;
        }
    }
    assert_eq!(size[0], 800, "{size}");
    assert!(size[1].as_f64().unwrap() < 600.0, "{size}");
    t.session.stop().await;
}

#[tokio::test]
async fn a_snapped_window_stays_within_the_work_area() {
    if std::env::var_os("TIZEN_SIM_TEST_HEADED").is_none() {
        // Only a window on a display has a work area to keep to.
        return;
    }
    let t = start("screen");
    t.session.wait_until_loaded().await.unwrap();
    let ws = debugger_url(&t.session).await;
    let window = try_cdp_call(&ws, None, "Browser.getWindowForTarget", target(&ws)).await;
    if window.is_err() && support::profile().version == "3.0" {
        // M47 centers the App instead of snapping the window.
        return;
    }
    let window = WindowControl {
        session: &t.session,
        ws: &ws,
        window_id: window.ok().map(|window| window["windowId"].clone()),
    };
    let area = window.work_area().await;
    let right = area[0] + area[2];
    // Small and near the work area's right edge (M56 doesn't move it quite
    // where asked), snapped to the App's 16:9.
    window.move_to(right - 900, area[1] + 10).await;
    window.resize(600, 380).await;
    tokio::time::sleep(Duration::from_millis(1500)).await;
    let left = window.position().await[0].as_i64().unwrap();
    let width = window.size().await[0].as_i64().unwrap();
    // Made taller, so its width follows: past the edge, whatever the frame.
    let height = (right - left + 300) * 9 / 16;
    assert!(height < area[3] - 20, "no room to try in {area:?}");
    window.resize(width as u32, height as u32).await;
    // The new size, then the snapped one.
    let (mut size, mut resized) = (Value::Null, false);
    for _ in 0..50 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        size = window.size().await;
        resized |= size == json!([width, height]);
        if resized && size != json!([width, height]) {
            break;
        }
    }
    assert!(resized, "never {width}x{height}: {size}");
    assert_ne!(size, json!([width, height]), "not snapped");
    let position = window.position().await;
    assert!(
        position[0].as_i64().unwrap() + size[0].as_i64().unwrap() <= right,
        "{position} {size} past the work area {area:?}"
    );
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

/// M63 and M69 drop the metrics override whenever their window is resized
/// (measured; M47, M56 and M76 on keep it), so the App would see the window's
/// own size, as one Nightly did 2 s after a resize. The TV screen is laid out
/// again as soon as the frame is resized, not at the next poll.
#[tokio::test]
async fn the_app_is_laid_out_again_as_soon_as_the_window_is_resized() {
    let t = start("screen");
    t.session.wait_until_loaded().await.unwrap();
    let ws = debugger_url(&t.session).await;
    // M47 takes one debugging client per page, the Session's.
    let window = try_cdp_call(&ws, None, "Browser.getWindowForTarget", target(&ws)).await;
    let window = WindowControl {
        session: &t.session,
        ws: &ws,
        window_id: window.ok().map(|window| window["windowId"].clone()),
    };
    let mut slowest = Duration::ZERO;
    // At 16:9, so the window isn't snapped in between.
    for (width, height) in [
        (1000, 600),
        (1200, 714),
        (900, 545),
        (1100, 657),
        (950, 573),
    ] {
        window.resize(width, height).await;
        let resized = std::time::Instant::now();
        loop {
            if t.session.evaluate(SIZE).await.unwrap() == json!([1920, 1080, 1920, 1080]) {
                break;
            }
            assert!(
                resized.elapsed() < Duration::from_secs(5),
                "never laid out again"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        slowest = slowest.max(resized.elapsed());
        tokio::time::sleep(Duration::from_millis(700)).await;
    }
    assert!(
        slowest < Duration::from_millis(300),
        "laid out again only after {slowest:?}"
    );
    t.session.stop().await;
}
