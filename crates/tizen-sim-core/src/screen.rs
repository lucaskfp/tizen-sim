//! The TV screen: the App laid out at the TV's resolution and scaled to fit
//! the Engine window, with transparent page areas shown black, or as a
//! checkerboard while debugging.
//!
//! - **Resolution.** The App's screen is 1920x1080 CSS px on UHD TVs and
//!   1280x720 on FHD TVs; larger content is scaled down to fit (TV
//!   guides/fundamentals/managing-screen-resolution.html;
//!   specifications/general-specifications.html lists "1920x1080 (All
//!   models)"). `screen.width`/`height` report the same size, since the
//!   resolution guide names them as how an App learns it. A custom viewport
//!   width from the App's viewport meta is not emulated.
//! - **Black.** That a TV shows transparent page areas black comes from the
//!   product spec (#23): no guide fetched so far says what is behind the
//!   page outside AVPlay's Video Plane (ADR-0004). **Unverified.**
//!
//! Only Chrome/130 has run this. Whether M47 has the CDP calls used here
//! (the `Browser` window methods, `setDefaultBackgroundColorOverride`,
//! `setDeviceMetricsOverride`'s `scale`) is **unverified** until an older
//! Version Profile's Engine is added (#38).

use crate::cdp::{Cdp, CdpError};
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Duration;

/// The App's screen size, which depends on the TV model group (TV
/// guides/fundamentals/managing-screen-resolution.html).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum AppResolution {
    /// 1920x1080, as on UHD TVs (the general specifications say all models).
    #[default]
    Uhd,
    /// 1280x720, as on FHD TVs.
    Fhd,
}

impl AppResolution {
    /// Width and height in CSS px.
    pub fn size(self) -> (u32, u32) {
        match self {
            AppResolution::Uhd => (1920, 1080),
            AppResolution::Fhd => (1280, 720),
        }
    }
}

/// The Engine window's content area a headed Session starts with: 720p,
/// which fits most desktops.
const INITIAL_CONTENT: (f64, f64) = (1280.0, 720.0);

/// How often the Engine window is checked for a new size.
const RESIZE_POLL: Duration = Duration::from_millis(500);

/// The scale that fits a `screen`-sized App into a `content`-sized window
/// area without cropping, up or down.
pub(crate) fn fit_scale(content: (f64, f64), screen: (u32, u32)) -> f64 {
    let scale = (content.0 / f64::from(screen.0)).min(content.1 / f64::from(screen.1));
    if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    }
}

/// Sets up the TV screen in the Engine's page target and keeps the App
/// scaled to the window as it is resized, until the Engine goes away.
/// `headed` windows are first resized to a 720p content area.
pub(crate) async fn set_up(
    cdp: &Arc<Cdp>,
    resolution: AppResolution,
    headed: bool,
) -> Result<(), CdpError> {
    // What is behind the page where it is transparent.
    if let Err(error) = cdp
        .call(
            "Emulation.setDefaultBackgroundColorOverride",
            json!({ "color": { "r": 0, "g": 0, "b": 0, "a": 1 } }),
        )
        .await
    {
        tracing::warn!(%error, "could not make transparent page areas black");
    }

    // The window's frame, measured before the override hides the real
    // viewport size from the page.
    let frame = cdp
        .evaluate("[outerWidth - innerWidth, outerHeight - innerHeight]")
        .await?;
    let frame = (
        frame[0].as_f64().unwrap_or(0.0),
        frame[1].as_f64().unwrap_or(0.0),
    );
    let window = match cdp.call("Browser.getWindowForTarget", json!({})).await {
        Ok(window) => Some(window),
        Err(error) => {
            // Without the Browser domain the window can't be measured, so
            // the App stays at its real size.
            tracing::warn!(%error, "could not find the Engine window; the App won't be scaled");
            None
        }
    };
    let Some(window) = window else {
        return apply(cdp, resolution, 1.0).await;
    };
    let window_id = window["windowId"].clone();
    let mut bounds = window["bounds"].clone();
    if headed {
        let (width, height) = INITIAL_CONTENT;
        let resized = cdp
            .call(
                "Browser.setWindowBounds",
                json!({
                    "windowId": window_id,
                    "bounds": { "width": width + frame.0, "height": height + frame.1 },
                }),
            )
            .await;
        match resized {
            Ok(_) => {
                bounds["width"] = json!(width + frame.0);
                bounds["height"] = json!(height + frame.1);
            }
            Err(error) => tracing::warn!(%error, "could not size the Engine window"),
        }
    }
    let content = move |bounds: &Value| {
        (
            bounds["width"].as_f64().unwrap_or(0.0) - frame.0,
            bounds["height"].as_f64().unwrap_or(0.0) - frame.1,
        )
    };
    let screen = resolution.size();
    apply(cdp, resolution, fit_scale(content(&bounds), screen)).await?;

    let cdp = cdp.clone();
    tokio::spawn(async move {
        let mut last = content(&bounds);
        loop {
            tokio::time::sleep(RESIZE_POLL).await;
            let Ok(window) = cdp
                .call("Browser.getWindowBounds", json!({ "windowId": window_id }))
                .await
            else {
                return;
            };
            let bounds = &window["bounds"];
            // A minimized (hidden) App keeps its scale for when it's back.
            if bounds["windowState"] == "minimized" {
                continue;
            }
            let now = content(bounds);
            if now != last {
                last = now;
                if apply(&cdp, resolution, fit_scale(now, screen))
                    .await
                    .is_err()
                {
                    return;
                }
            }
        }
    });
    Ok(())
}

/// Lays the page out at the TV's resolution, drawn at `scale`.
async fn apply(cdp: &Cdp, resolution: AppResolution, scale: f64) -> Result<(), CdpError> {
    let (width, height) = resolution.size();
    cdp.call(
        "Emulation.setDeviceMetricsOverride",
        json!({
            "width": width,
            "height": height,
            "screenWidth": width,
            "screenHeight": height,
            "deviceScaleFactor": 1,
            "mobile": false,
            "scale": scale,
        }),
    )
    .await?;
    Ok(())
}

/// Shows or hides the checkerboard behind the page in the loaded document.
/// It has to be shown again after each navigation.
pub(crate) async fn show_checkerboard(cdp: &Cdp, on: bool) -> Result<(), CdpError> {
    cdp.evaluate(&format!("({CHECKERBOARD})({on})")).await?;
    Ok(())
}

/// Puts a checkerboard layer at the back of the page's root stacking
/// context, where ADR-0004 puts the Video Plane, so App content covers it
/// exactly as it would cover the TV's black. The root (canvas) background
/// paints beneath any element, so the layer redraws that background over
/// its checks while it is a colour that lets them show, and hides while the
/// root background is opaque or an image (which may or may not be opaque;
/// it is taken as opaque). Re-checked as the App restyles. ES5, for old
/// Engines.
const CHECKERBOARD: &str = r#"function (on) {
  var host = window.__TIZEN_SIM__ || (window.__TIZEN_SIM__ = {});
  var state = host.checkerboard || (host.checkerboard = {});
  if (state.timer) clearInterval(state.timer);
  state.timer = 0;
  if (state.layer && state.layer.parentNode) state.layer.parentNode.removeChild(state.layer);
  if (!on) return 0;
  var checks = 'linear-gradient(45deg, #ccc 25%, transparent 25%, transparent 75%, #ccc 75%)';
  var layer = state.layer = document.createElement('div');
  layer.setAttribute('data-tizen-sim', 'checkerboard');
  layer.style.cssText = 'position:fixed;left:0;top:0;width:100%;height:100%;' +
    'z-index:-2147483648;pointer-events:none;background-color:#fff;' +
    'background-size:auto,32px 32px,32px 32px;background-position:0 0,0 0,16px 16px';
  function transparentColor(style) {
    if (style.backgroundImage !== 'none') return null;
    var color = style.backgroundColor;
    var rgba = /^rgba\(.*,\s*([\d.]+)\)$/.exec(color);
    return rgba && parseFloat(rgba[1]) < 1 ? color : null;
  }
  function rootBackground() {
    var html = getComputedStyle(document.documentElement);
    var color = transparentColor(html);
    // An html element with no background takes the body's (CSS backgrounds propagate).
    if (color && /,\s*0\)$/.test(color) && document.body) {
      return transparentColor(getComputedStyle(document.body));
    }
    return color;
  }
  function paint() {
    var root = document.documentElement;
    if (!root) return;
    if (layer.parentNode !== root) root.appendChild(layer);
    var color = rootBackground();
    layer.style.display = color ? 'block' : 'none';
    if (color) {
      layer.style.backgroundImage =
        'linear-gradient(' + color + ', ' + color + '), ' + checks + ', ' + checks;
    }
  }
  paint();
  state.timer = setInterval(paint, 250);
  return 1;
}"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scales_to_fit_the_window_without_cropping() {
        assert_eq!(fit_scale((960.0, 540.0), (1920, 1080)), 0.5);
        // Too tall a window: the width decides.
        assert_eq!(fit_scale((960.0, 900.0), (1920, 1080)), 0.5);
        // Too wide: the height decides.
        assert_eq!(fit_scale((3000.0, 540.0), (1920, 1080)), 0.5);
        assert_eq!(fit_scale((1280.0, 720.0), (1280, 720)), 1.0);
        assert_eq!(fit_scale((3840.0, 2160.0), (1920, 1080)), 2.0);
    }

    #[test]
    fn an_empty_window_leaves_the_scale_alone() {
        assert_eq!(fit_scale((0.0, 0.0), (1920, 1080)), 1.0);
        assert_eq!(fit_scale((-10.0, 500.0), (1920, 1080)), 1.0);
    }
}
