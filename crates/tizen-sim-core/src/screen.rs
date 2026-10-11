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
//! Old Engines lack some of the protocol used here, so each part has a
//! fallback, measured on M47 (protocol 1.1):
//!
//! - Without the `Browser` domain, the window is measured from the page
//!   (`outerWidth`/`outerHeight`, which the metrics override leaves alone)
//!   and not resized.
//! - Without `screenWidth`/`screenHeight` in the metrics override, the Shims
//!   report the screen size (see `shim::session_data`).
//! - Without `setDefaultBackgroundColorOverride`, a black layer at the back
//!   of the page stands in for the TV's black, like the checkerboard.
//! - `Page.captureScreenshot` gives the window's content, not the emulated
//!   screen, so `Screen::fit_screenshot` cuts the App out and scales it.

use crate::cdp::{Cdp, CdpError};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
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

/// What is behind the page where it is transparent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Backdrop {
    /// The TV's black, drawn by the Engine itself.
    Engine,
    /// The TV's black, drawn by a layer at the back of the page.
    BlackLayer,
    /// The transparency checkerboard.
    Checkerboard,
}

/// A Session's TV screen, as set up by `set_up`.
pub(crate) struct Screen {
    resolution: AppResolution,
    /// Whether the Engine draws transparent areas black itself.
    engine_black: bool,
    /// How the App was last fitted to the window.
    fit: Mutex<Fit>,
}

/// The window's content area, in CSS px, and the App's scale in it.
#[derive(Debug, Clone, Copy)]
struct Fit {
    content: (f64, f64),
    scale: f64,
}

impl Screen {
    /// The backdrop to show: the checkerboard while it is on, else black.
    pub(crate) fn backdrop(&self, checkerboard: bool) -> Backdrop {
        match (checkerboard, self.engine_black) {
            (true, _) => Backdrop::Checkerboard,
            (false, true) => Backdrop::Engine,
            (false, false) => Backdrop::BlackLayer,
        }
    }

    /// A screenshot of the TV screen from what `Page.captureScreenshot`
    /// returned: as is if it is the screen, otherwise (old Engines capture the
    /// window's content) the App's area cut out and scaled to the screen.
    pub(crate) fn fit_screenshot(&self, png: Vec<u8>) -> Result<Vec<u8>, String> {
        let (width, height) = self.resolution.size();
        let image = image::load_from_memory_with_format(&png, image::ImageFormat::Png)
            .map_err(|e| format!("the screenshot is not a valid PNG: {e}"))?;
        if (image.width(), image.height()) == (width, height) {
            return Ok(png);
        }
        let Fit { content, scale } = *self.fit.lock().unwrap();
        // The App is drawn at the top left of the content area, sized in CSS
        // px; the image may be in device pixels.
        let cut = |pixels: u32, css: f64, screen: u32| {
            let share = (f64::from(screen) * scale / css).clamp(0.0, 1.0);
            ((f64::from(pixels) * share).round() as u32).clamp(1, pixels)
        };
        let app = image.crop_imm(
            0,
            0,
            cut(image.width(), content.0, width),
            cut(image.height(), content.1, height),
        );
        let mut out = Vec::new();
        app.resize_exact(width, height, image::imageops::FilterType::Triangle)
            .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
            .map_err(|e| format!("could not encode the screenshot: {e}"))?;
        Ok(out)
    }

    /// Fits the App to a window content area.
    async fn fit(&self, cdp: &Cdp, content: (f64, f64)) -> Result<(), CdpError> {
        let scale = fit_scale(content, self.resolution.size());
        *self.fit.lock().unwrap() = Fit { content, scale };
        apply(cdp, self.resolution, scale).await
    }
}

/// How the Engine window's size is learned.
enum WindowSource {
    /// From the `Browser` domain, by window id.
    Browser(Value),
    /// From the page, on Engines without the `Browser` domain.
    Page,
}

impl WindowSource {
    /// The window's content area, given its `frame`; `None` while it is
    /// minimized (the App is hidden), when the App keeps its scale for when
    /// it's back.
    async fn content(&self, cdp: &Cdp, frame: (f64, f64)) -> Result<Option<(f64, f64)>, CdpError> {
        let (width, height) = match self {
            WindowSource::Browser(window_id) => {
                let window = cdp
                    .call("Browser.getWindowBounds", json!({ "windowId": window_id }))
                    .await?;
                let bounds = &window["bounds"];
                if bounds["windowState"] == "minimized" {
                    return Ok(None);
                }
                (bounds["width"].as_f64(), bounds["height"].as_f64())
            }
            WindowSource::Page => {
                let outer = cdp
                    .evaluate("[outerWidth, outerHeight, document.hidden]")
                    .await?;
                if outer[2] == true {
                    return Ok(None);
                }
                (outer[0].as_f64(), outer[1].as_f64())
            }
        };
        Ok(Some((
            width.unwrap_or(0.0) - frame.0,
            height.unwrap_or(0.0) - frame.1,
        )))
    }
}

/// Sets up the TV screen in the Engine's page target and keeps the App
/// scaled to the window as it is resized, until the Engine goes away.
/// `headed` windows are first resized to a 720p content area, where the
/// Engine can.
pub(crate) async fn set_up(
    cdp: &Arc<Cdp>,
    resolution: AppResolution,
    headed: bool,
) -> Result<Arc<Screen>, CdpError> {
    let engine_black = match cdp
        .call(
            "Emulation.setDefaultBackgroundColorOverride",
            json!({ "color": { "r": 0, "g": 0, "b": 0, "a": 1 } }),
        )
        .await
    {
        Ok(_) => true,
        Err(error) => {
            tracing::debug!(%error, "transparent page areas are made black by a layer instead");
            false
        }
    };

    // The window's frame and content, measured before the override hides the
    // real viewport size from the page.
    let measured = cdp
        .evaluate("[outerWidth - innerWidth, outerHeight - innerHeight, innerWidth, innerHeight]")
        .await?;
    let number = |i: usize| measured[i].as_f64().unwrap_or(0.0);
    let frame = (number(0), number(1));
    let mut content = (number(2), number(3));
    let source = match cdp.call("Browser.getWindowForTarget", json!({})).await {
        Ok(window) => WindowSource::Browser(window["windowId"].clone()),
        Err(error) => {
            tracing::debug!(%error, "no Browser domain; the window is measured from the page");
            WindowSource::Page
        }
    };
    if let (true, WindowSource::Browser(window_id)) = (headed, &source) {
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
            Ok(_) => content = INITIAL_CONTENT,
            Err(error) => tracing::warn!(%error, "could not size the Engine window"),
        }
    }
    let screen = Arc::new(Screen {
        resolution,
        engine_black,
        fit: Mutex::new(Fit {
            content,
            scale: 1.0,
        }),
    });
    screen.fit(cdp, content).await?;

    let (cdp, watched) = (cdp.clone(), screen.clone());
    tokio::spawn(async move {
        let mut last = content;
        loop {
            tokio::time::sleep(RESIZE_POLL).await;
            let now = match source.content(&cdp, frame).await {
                Ok(Some(now)) => now,
                Ok(None) => continue,
                Err(CdpError::Closed | CdpError::Connect(_)) => return,
                // The Browser domain fails once the Engine is going; the page
                // also fails while it navigates.
                Err(_) if matches!(source, WindowSource::Browser(_)) => return,
                Err(_) => continue,
            };
            if now != last {
                last = now;
                if watched.fit(&cdp, now).await.is_err() {
                    return;
                }
            }
        }
    });
    Ok(screen)
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
            // Required by M47 (protocol 1.1); newer Engines ignore it.
            "fitWindow": false,
        }),
    )
    .await?;
    Ok(())
}

/// Shows a backdrop behind the page in the loaded document. It has to be
/// shown again after each navigation.
pub(crate) async fn show_backdrop(cdp: &Cdp, backdrop: Backdrop) -> Result<(), CdpError> {
    let mode = match backdrop {
        Backdrop::Engine => "none",
        Backdrop::BlackLayer => "black",
        Backdrop::Checkerboard => "checks",
    };
    cdp.evaluate(&format!("({BACKDROP})('{mode}')")).await?;
    Ok(())
}

/// Puts a layer at the back of the page's root stacking context, where
/// ADR-0004 puts the Video Plane, so App content covers it exactly as it
/// would cover the TV's black: the checkerboard (`checks`), black where the
/// Engine can't make it so itself (`black`), or nothing (`none`). The root
/// (canvas) background paints beneath any element, so the layer redraws that
/// background over itself while it is a colour that lets it show, and hides
/// while the root background is opaque or an image (which may or may not be
/// opaque; it is taken as opaque). Re-checked as the App restyles. ES5, for
/// old Engines.
const BACKDROP: &str = r#"function (mode) {
  var host = window.__TIZEN_SIM__ || (window.__TIZEN_SIM__ = {});
  var state = host.backdrop || (host.backdrop = {});
  if (state.timer) clearInterval(state.timer);
  state.timer = 0;
  if (state.layer && state.layer.parentNode) state.layer.parentNode.removeChild(state.layer);
  if (mode === 'none') return 0;
  var checks = 'linear-gradient(45deg, #ccc 25%, transparent 25%, transparent 75%, #ccc 75%)';
  var below = mode === 'checks' ? ', ' + checks + ', ' + checks : '';
  var layer = state.layer = document.createElement('div');
  layer.setAttribute('data-tizen-sim', mode === 'checks' ? 'checkerboard' : 'backdrop');
  layer.style.cssText = 'position:fixed;left:0;top:0;width:100%;height:100%;' +
    'z-index:-2147483648;pointer-events:none;' +
    'background-color:' + (mode === 'checks' ? '#fff' : '#000') + ';' +
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
      layer.style.backgroundImage = 'linear-gradient(' + color + ', ' + color + ')' + below;
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

    fn png(width: u32, height: u32, color: impl Fn(u32, u32) -> [u8; 3]) -> Vec<u8> {
        let image = image::RgbImage::from_fn(width, height, |x, y| image::Rgb(color(x, y)));
        let mut out = Vec::new();
        image
            .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
            .unwrap();
        out
    }

    fn screen(content: (f64, f64)) -> Screen {
        Screen {
            resolution: AppResolution::Uhd,
            engine_black: false,
            fit: Mutex::new(Fit {
                content,
                scale: fit_scale(content, (1920, 1080)),
            }),
        }
    }

    #[test]
    fn a_screenshot_of_the_screen_is_kept() {
        let shot = png(1920, 1080, |_, _| [1, 2, 3]);
        assert_eq!(
            screen((960.0, 540.0)).fit_screenshot(shot.clone()).unwrap(),
            shot
        );
    }

    #[test]
    fn a_screenshot_of_the_window_is_cut_to_the_app_and_scaled() {
        // A 960x600 content area holds the App at 0.5 scale, 960x540, at the
        // top left; the 60px strip below it isn't part of the TV screen.
        let shot = png(
            960,
            600,
            |_, y| if y < 540 { [255, 0, 0] } else { [0, 0, 255] },
        );
        let fitted = screen((960.0, 600.0)).fit_screenshot(shot).unwrap();
        let fitted = image::load_from_memory(&fitted).unwrap().to_rgb8();
        assert_eq!(fitted.dimensions(), (1920, 1080));
        assert_eq!(fitted.get_pixel(960, 1079).0, [255, 0, 0]);
    }

    #[test]
    fn a_screenshot_in_device_pixels_is_cut_in_proportion() {
        // The same window at 200% display scaling.
        let shot = png(
            1920,
            1200,
            |_, y| if y < 1080 { [255, 0, 0] } else { [0, 0, 255] },
        );
        let fitted = screen((960.0, 600.0)).fit_screenshot(shot).unwrap();
        let fitted = image::load_from_memory(&fitted).unwrap().to_rgb8();
        assert_eq!(fitted.get_pixel(1919, 1079).0, [255, 0, 0]);
    }
}
