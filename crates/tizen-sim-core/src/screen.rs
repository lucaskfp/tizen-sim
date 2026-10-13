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
//! - **Centered.** The TV screen sits in the middle of the window, with the
//!   leftover space as a letterbox (#57). Only M47 can draw the App off the
//!   window's top left (`offsetX`/`offsetY`); from M63 on the metrics
//!   override's `viewport` was measured not to shift it right or down
//!   (negative positions do nothing). So elsewhere the window is snapped to
//!   the App's aspect once the user resizes it, which a maximized or full
//!   screen window can't be, and M56 (neither) keeps the App at the top left.
//!
//! Old Engines lack some of the protocol used here, so each part has a
//! fallback, measured on M47 (protocol 1.1):
//!
//! - Without the `Browser` domain (M47, M56), the window is measured from
//!   the page (`outerWidth`/`outerHeight`, which the metrics override leaves
//!   alone) and not resized.
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
    pub const fn size(self) -> (u32, u32) {
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

/// How far, in CSS px, a window's content area may be off the App's aspect
/// before it is snapped: rounding, such as the GUI's 16:9 box, is left alone.
const SNAP_TOLERANCE: f64 = 2.0;

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

/// Where a `screen`-sized App fitted to a `content`-sized area goes for it
/// to be centered: half the leftover space on each side.
fn centered(content: (f64, f64), screen: (u32, u32)) -> (f64, f64) {
    let scale = fit_scale(content, screen);
    (
        ((content.0 - f64::from(screen.0) * scale) / 2.0).max(0.0),
        ((content.1 - f64::from(screen.1) * scale) / 2.0).max(0.0),
    )
}

/// The content area a window resized from `before` to `after` is snapped to,
/// so it holds the App with no leftover space; `None` when it already does.
/// The side the user dragged is kept and the other follows; when both
/// changed, the window shrinks around the App.
fn snap(before: (f64, f64), after: (f64, f64), screen: (u32, u32)) -> Option<(f64, f64)> {
    if after.0 < 1.0 || after.1 < 1.0 {
        return None;
    }
    let scale = fit_scale(after, screen);
    let (width, height) = (f64::from(screen.0), f64::from(screen.1));
    if after.0 - width * scale <= SNAP_TOLERANCE && after.1 - height * scale <= SNAP_TOLERANCE {
        return None;
    }
    let target = if after.1 == before.1 {
        (after.0, after.0 * height / width)
    } else if after.0 == before.0 {
        (after.1 * width / height, after.1)
    } else {
        (width * scale, height * scale)
    };
    Some((target.0.round(), target.1.round()))
}

/// Whether an Engine, by its `/json/version` browser name, draws the App at
/// the offset `Emulation.setDeviceMetricsOverride` gives it. Measured: M47
/// (protocol 1.1) does; M56 ignores `offsetX`/`offsetY`, and M63 and later
/// no longer have them.
pub(crate) fn draws_at_offset(engine_version: &str) -> bool {
    let milestone = engine_version
        .split_once('/')
        .and_then(|(_, version)| version.split('.').next())
        .and_then(|major| major.parse::<u32>().ok());
    milestone.is_some_and(|milestone| milestone <= 47)
}

/// How the App is drawn in the window's content area.
#[derive(Debug, Clone, Copy)]
struct Fit {
    scale: f64,
    /// Where its top left corner is, in CSS px of the window.
    offset: (f64, f64),
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
    /// Device pixels per CSS px of the Engine window, before the metrics
    /// override sets its own: the scale of a screenshot of the window's
    /// content. Only measured at 100% desktop scaling (1); that the image
    /// follows a larger ratio is **unverified**.
    pixel_ratio: f64,
    /// Whether the Engine centers the App in the window (`draws_at_offset`);
    /// otherwise it is drawn at the top left, and the window is snapped to
    /// it instead where the Engine can be resized.
    draws_at_offset: bool,
    /// The App in the window, as last fitted.
    fit: Mutex<Fit>,
}

impl Screen {
    /// Has an Engine that draws transparent areas black do so again in a
    /// new document, also under the checkerboard, for when it is turned off.
    pub(crate) async fn restore_engine_black(&self, cdp: &Cdp) -> Result<(), CdpError> {
        if self.engine_black {
            paint_transparent_areas_black(cdp).await?;
        }
        Ok(())
    }

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
        let Fit { scale, offset } = *self.fit.lock().unwrap();
        // The App is drawn at its offset in the window's content, at its
        // scale, and the image is in device pixels. The content may be larger
        // than the Session believes: embedded in the GUI, the window keeps
        // the room of the frame it lost (ADR-0006), past the App (#60).
        let cut = |pixels: u32, screen: u32, offset: f64| {
            let start = ((offset * self.pixel_ratio).round() as u32).min(pixels - 1);
            let drawn = f64::from(screen) * scale * self.pixel_ratio;
            (start, (drawn.round() as u32).clamp(1, pixels - start))
        };
        let (x, app_width) = cut(image.width(), width, offset.0);
        let (y, app_height) = cut(image.height(), height, offset.1);
        let app = image.crop_imm(x, y, app_width, app_height);
        let mut out = Vec::new();
        app.resize_exact(width, height, image::imageops::FilterType::Triangle)
            .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
            .map_err(|e| format!("could not encode the screenshot: {e}"))?;
        Ok(out)
    }

    /// Fits the App to a window content area, centered where the Engine can.
    async fn fit(&self, cdp: &Cdp, content: (f64, f64)) -> Result<(), CdpError> {
        let screen = self.resolution.size();
        let fit = Fit {
            scale: fit_scale(content, screen),
            offset: if self.draws_at_offset {
                centered(content, screen)
            } else {
                (0.0, 0.0)
            },
        };
        *self.fit.lock().unwrap() = fit;
        apply(cdp, self.resolution, fit).await
    }
}

/// How the Engine window's size is learned.
enum WindowSource {
    /// From the `Browser` domain, by window id.
    Browser(Value),
    /// From the page, on Engines without the `Browser` domain.
    Page,
}

/// The Engine window as last measured.
struct Window {
    /// Its content area.
    content: (f64, f64),
    /// Whether it can be resized: neither maximized nor full screen, and
    /// measured through the `Browser` domain, which can resize it.
    resizable: bool,
}

impl WindowSource {
    /// The window, given its `frame`; `None` while it is minimized (the App
    /// is hidden), when the App keeps its scale for when it's back.
    async fn window(&self, cdp: &Cdp, frame: (f64, f64)) -> Result<Option<Window>, CdpError> {
        let (width, height, resizable) = match self {
            WindowSource::Browser(window_id) => {
                let window = cdp
                    .call("Browser.getWindowBounds", json!({ "windowId": window_id }))
                    .await?;
                let bounds = &window["bounds"];
                if bounds["windowState"] == "minimized" {
                    return Ok(None);
                }
                let normal = bounds["windowState"] == "normal";
                (bounds["width"].as_f64(), bounds["height"].as_f64(), normal)
            }
            WindowSource::Page => {
                let outer = cdp
                    .evaluate("[outerWidth, outerHeight, document.hidden]")
                    .await?;
                if outer[2] == true {
                    return Ok(None);
                }
                (outer[0].as_f64(), outer[1].as_f64(), false)
            }
        };
        Ok(Some(Window {
            content: (
                width.unwrap_or(0.0) - frame.0,
                height.unwrap_or(0.0) - frame.1,
            ),
            resizable,
        }))
    }

    /// Resizes the window to a `content`-sized content area, given its
    /// `frame`, where the Engine can.
    async fn resize(
        &self,
        cdp: &Cdp,
        frame: (f64, f64),
        content: (f64, f64),
    ) -> Result<(), CdpError> {
        if let WindowSource::Browser(window_id) = self {
            // Whole pixels: M63 refuses `800.0`.
            let pixels = |size: f64| size.round() as i64;
            cdp.call(
                "Browser.setWindowBounds",
                json!({
                    "windowId": window_id,
                    "bounds": {
                        "width": pixels(content.0 + frame.0),
                        "height": pixels(content.1 + frame.1),
                    },
                }),
            )
            .await?;
        }
        Ok(())
    }
}

/// The Engine's page target: its id (the last part of its debugging
/// address) and the Engine's `/json/version` browser name.
pub(crate) struct PageTarget<'a> {
    pub id: &'a str,
    pub engine_version: &'a str,
}

/// Sets up the TV screen in the Engine's page target and keeps the App
/// scaled to the window and centered in it as it is resized, until the
/// Engine goes away. `headed` windows are first resized to a 720p content
/// area, where the Engine can.
///
/// Only the oldest Engines can draw the App off the window's top left
/// (`draws_at_offset`); for the others the window is snapped to the App's
/// aspect once the user has resized it, while it is neither maximized nor
/// full screen (#57).
pub(crate) async fn set_up(
    cdp: &Arc<Cdp>,
    target: PageTarget<'_>,
    resolution: AppResolution,
    headed: bool,
) -> Result<Arc<Screen>, CdpError> {
    let engine_black = match paint_transparent_areas_black(cdp).await {
        Ok(()) => true,
        Err(error) => {
            tracing::debug!(%error, "transparent page areas are made black by a layer instead");
            false
        }
    };

    // The window's frame, content and pixel ratio, measured before the
    // override hides the real viewport from the page.
    let measured = cdp
        .evaluate(
            "[outerWidth - innerWidth, outerHeight - innerHeight, innerWidth, innerHeight,              devicePixelRatio]",
        )
        .await?;
    let number = |i: usize| measured[i].as_f64().unwrap_or(0.0);
    let frame = (number(0), number(1));
    let mut content = (number(2), number(3));
    let pixel_ratio = Some(number(4)).filter(|ratio| *ratio > 0.0).unwrap_or(1.0);
    // M63 requires the target; later Engines default to the caller's.
    let window_for_target = json!({ "targetId": target.id });
    let source = match cdp
        .call("Browser.getWindowForTarget", window_for_target)
        .await
    {
        Ok(window) => WindowSource::Browser(window["windowId"].clone()),
        Err(error) => {
            tracing::debug!(%error, "no Browser domain; the window is measured from the page");
            WindowSource::Page
        }
    };
    if let (true, WindowSource::Browser(_)) = (headed, &source) {
        match source.resize(cdp, frame, INITIAL_CONTENT).await {
            Ok(()) => content = INITIAL_CONTENT,
            Err(error) => tracing::warn!(%error, "could not size the Engine window"),
        }
    }
    let draws_at_offset = draws_at_offset(target.engine_version);
    let screen = Arc::new(Screen {
        resolution,
        engine_black,
        pixel_ratio,
        draws_at_offset,
        fit: Mutex::new(Fit {
            scale: 1.0,
            offset: (0.0, 0.0),
        }),
    });
    screen.fit(cdp, content).await?;

    let (cdp, watched) = (cdp.clone(), screen.clone());
    tokio::spawn(async move {
        // The size the App was last fitted to, and the last one the window
        // settled at: it is snapped only once its size stops changing, so a
        // resize the user is still dragging isn't fought.
        let (mut last, mut settled) = (content, content);
        loop {
            tokio::time::sleep(RESIZE_POLL).await;
            let window = match source.window(&cdp, frame).await {
                Ok(Some(window)) => window,
                Ok(None) => continue,
                Err(CdpError::Closed | CdpError::Connect(_)) => return,
                // The Browser domain fails once the Engine is going; the page
                // also fails while it navigates.
                Err(_) if matches!(source, WindowSource::Browser(_)) => return,
                Err(_) => continue,
            };
            let now = window.content;
            if now != last {
                last = now;
                if watched.fit(&cdp, now).await.is_err() {
                    return;
                }
                continue;
            }
            if now == settled {
                continue;
            }
            let before = std::mem::replace(&mut settled, now);
            if draws_at_offset || !window.resizable {
                continue;
            }
            if let Some(snapped) = snap(before, now, resolution.size())
                && let Err(error) = source.resize(&cdp, frame, snapped).await
            {
                tracing::debug!(%error, "could not snap the Engine window to the App");
            }
        }
    });
    Ok(screen)
}

/// Lays the page out at the TV's resolution, drawn as `fit` says.
async fn apply(cdp: &Cdp, resolution: AppResolution, fit: Fit) -> Result<(), CdpError> {
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
            "scale": fit.scale,
            // Required by M47 (protocol 1.1); newer Engines ignore it.
            "fitWindow": false,
            // Only M47 has these (see `draws_at_offset`).
            "offsetX": fit.offset.0,
            "offsetY": fit.offset.1,
        }),
    )
    .await?;
    Ok(())
}

/// Has the Engine draw transparent page areas black. M63 forgets this when a
/// navigation moves the page to another renderer process, so it is asked
/// again in each new document (measured; M69 keeps it).
async fn paint_transparent_areas_black(cdp: &Cdp) -> Result<(), CdpError> {
    cdp.call(
        "Emulation.setDefaultBackgroundColorOverride",
        json!({ "color": { "r": 0, "g": 0, "b": 0, "a": 1 } }),
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

    #[test]
    fn the_app_is_centered_in_the_leftover_space() {
        // 960x540 of App in a 1160x540 area: 100px each side.
        assert_eq!(centered((1160.0, 540.0), (1920, 1080)), (100.0, 0.0));
        // In a 960x600 one: 30px above and below.
        assert_eq!(centered((960.0, 600.0), (1920, 1080)), (0.0, 30.0));
        assert_eq!(centered((960.0, 540.0), (1920, 1080)), (0.0, 0.0));
    }

    const UHD: (u32, u32) = AppResolution::Uhd.size();

    #[test]
    fn a_window_already_at_the_apps_aspect_is_not_snapped() {
        assert_eq!(snap((1280.0, 720.0), (960.0, 540.0), UHD), None);
        // Off by a pixel or two from rounding, such as the GUI's 16:9 box.
        assert_eq!(snap((1280.0, 720.0), (961.0, 540.0), UHD), None);
        assert_eq!(snap((1280.0, 720.0), (960.0, 542.0), UHD), None);
    }

    #[test]
    fn a_window_resized_by_its_width_follows_with_its_height() {
        assert_eq!(
            snap((1280.0, 720.0), (1600.0, 720.0), UHD),
            Some((1600.0, 900.0))
        );
        assert_eq!(
            snap((1280.0, 720.0), (960.0, 720.0), UHD),
            Some((960.0, 540.0))
        );
    }

    #[test]
    fn a_window_resized_by_its_height_follows_with_its_width() {
        assert_eq!(
            snap((1280.0, 720.0), (1280.0, 900.0), UHD),
            Some((1600.0, 900.0))
        );
        assert_eq!(
            snap((1280.0, 720.0), (1280.0, 540.0), UHD),
            Some((960.0, 540.0))
        );
    }

    #[test]
    fn a_window_resized_both_ways_shrinks_to_the_app() {
        assert_eq!(
            snap((1280.0, 720.0), (800.0, 600.0), UHD),
            Some((800.0, 450.0))
        );
        assert_eq!(
            snap((1280.0, 720.0), (1500.0, 661.0), UHD),
            Some((1175.0, 661.0))
        );
        // Whole pixels.
        assert_eq!(
            snap((1280.0, 720.0), (1001.0, 1000.0), UHD),
            Some((1001.0, 563.0))
        );
    }

    #[test]
    fn an_empty_window_is_not_snapped() {
        assert_eq!(snap((1280.0, 720.0), (0.0, 0.0), UHD), None);
    }

    #[test]
    fn offsets_are_drawn_by_the_oldest_engines_only() {
        assert!(draws_at_offset("Chrome/47.0.2526.0"));
        assert!(!draws_at_offset("Chrome/56.0.2924.0"));
        assert!(!draws_at_offset("HeadlessChrome/130.0.6723.116"));
        assert!(!draws_at_offset("Chrome/130.0.6723.116"));
        assert!(!draws_at_offset("something else"));
    }

    fn png(width: u32, height: u32, color: impl Fn(u32, u32) -> [u8; 3]) -> Vec<u8> {
        let image = image::RgbImage::from_fn(width, height, |x, y| image::Rgb(color(x, y)));
        let mut out = Vec::new();
        image
            .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
            .unwrap();
        out
    }

    /// A Screen whose App was fitted to a `content`-sized window area, with
    /// `pixel_ratio` device pixels per CSS px, by an Engine that draws it at
    /// the top left.
    fn screen(content: (f64, f64), pixel_ratio: f64) -> Screen {
        screen_drawing_at(content, pixel_ratio, (0.0, 0.0))
    }

    fn screen_drawing_at(content: (f64, f64), pixel_ratio: f64, offset: (f64, f64)) -> Screen {
        Screen {
            resolution: AppResolution::Uhd,
            engine_black: false,
            pixel_ratio,
            draws_at_offset: offset != (0.0, 0.0),
            fit: Mutex::new(Fit {
                scale: fit_scale(content, (1920, 1080)),
                offset,
            }),
        }
    }

    #[test]
    fn a_screenshot_of_a_centered_app_is_cut_where_it_is_drawn() {
        // M47 draws the App centered: 960x540 in a 1160x540 area, 100px from
        // the left.
        let shot = png(1160, 540, |x, _| {
            if (100..1060).contains(&x) {
                [255, 0, 0]
            } else {
                [0, 0, 255]
            }
        });
        let fitted = screen_drawing_at((1160.0, 540.0), 1.0, (100.0, 0.0))
            .fit_screenshot(shot)
            .unwrap();
        let fitted = image::load_from_memory(&fitted).unwrap().to_rgb8();
        for (x, y) in [(0, 0), (1919, 0), (0, 1079), (1919, 1079), (960, 540)] {
            assert_eq!(fitted.get_pixel(x, y).0, [255, 0, 0], "at {x},{y}");
        }
    }

    #[test]
    fn a_screenshot_of_the_screen_is_kept() {
        let shot = png(1920, 1080, |_, _| [1, 2, 3]);
        assert_eq!(
            screen((960.0, 540.0), 1.0)
                .fit_screenshot(shot.clone())
                .unwrap(),
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
        let fitted = screen((960.0, 600.0), 1.0).fit_screenshot(shot).unwrap();
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
        let fitted = screen((960.0, 600.0), 2.0).fit_screenshot(shot).unwrap();
        let fitted = image::load_from_memory(&fitted).unwrap().to_rgb8();
        assert_eq!(fitted.get_pixel(1919, 1079).0, [255, 0, 0]);
    }

    #[test]
    fn a_window_larger_than_believed_is_cut_to_the_app() {
        // Embedded in the GUI, the window keeps the room of its lost frame:
        // the App is fitted to 1280x720, the content is 1296x759 (#60).
        let shot = png(1296, 759, |x, y| {
            if x < 1280 && y < 720 {
                [255, 0, 0]
            } else {
                [128, 128, 128]
            }
        });
        let fitted = screen((1280.0, 720.0), 1.0).fit_screenshot(shot).unwrap();
        let fitted = image::load_from_memory(&fitted).unwrap().to_rgb8();
        assert_eq!(fitted.get_pixel(1919, 1079).0, [255, 0, 0]);
    }
}
