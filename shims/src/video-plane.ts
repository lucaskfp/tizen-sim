// The Video Plane: where AVPlay's video appears (ADR-0004). On a TV it is a
// hardware layer behind the web page; here it is the back-most element of the
// page's root stacking context, just above the black backdrop (see
// crates/tizen-sim-core/src/screen.rs), so the browser's own paint order makes
// App content cover it as it would on a TV.
//
// - The display area comes from setDisplayRect, always in 1920x1080
//   coordinates whatever the App's resolution (avplay-api.html; TV
//   guides/multimedia/media-playback/using-avplay.html).
// - It shows real playback in a <video> when the Engine can decode the
//   stream, and a test pattern otherwise. Old Engines lack H.264/AAC and none
//   play HLS or DASH, so the pattern is the common case.
// - The root (canvas) background paints beneath every element, so the plane
//   checks it: an opaque one hides the plane and raises the
//   `video-plane-hidden` Diagnostic; a translucent one is redrawn over the
//   video, as the TV blends the page over it. Only colours are checked: a
//   background image may or may not be opaque.
// - Its <video> loads the stream like any page request, so the Access Policy
//   applies to it. Whether a TV applies it to AVPlay's own player is
//   unverified.

/** How often the canvas background is checked while the plane is shown. */
var CANVAS_CHECK_MS = 250;

/** How long a stream gets to show it decodes before the test pattern shows. */
var DECODE_PROBE_MS = 5000;

// SMPTE colour bars at 75%.
var PATTERN_BARS = ["#c0c0c0", "#c0c000", "#00c0c0", "#00c000", "#c000c0", "#c00000", "#0000c0"];

interface DisplayRect {
  x: number;
  y: number;
  width: number;
  height: number;
}

interface VideoPlane {
  /**
   * Starts loading `url`. `decodable` gets the <video> once the Engine can
   * play it (possibly after the probe gave up); `settled` is called once,
   * when that is known or the probe gives up.
   */
  load(url: string, decodable: (video: HTMLVideoElement) => void, settled: () => void): void;
  /** Drops the stream, leaving the plane empty. */
  unload(): void;
  /** Shows the plane at `rect` (1920x1080 coordinates), or hides it with null. */
  show(rect: DisplayRect | null): void;
  /** How the video fits the display area: CSS object-fit. */
  fit(objectFit: string): void;
  /** The line under the test pattern. */
  caption(text: string): void;
}

function createVideoPlane(): VideoPlane {
  var plane = document.createElement("div");
  plane.setAttribute("data-tizen-sim", "video-plane");
  // One above the backdrop's z-index, so the backdrop stays behind it.
  plane.style.cssText =
    "position:fixed;z-index:-2147483647;pointer-events:none;overflow:hidden;background:#000;display:none";

  var pattern = document.createElement("div");
  var stops: string[] = [];
  for (var i = 0; i < PATTERN_BARS.length; i++) {
    var from = ((i * 100) / PATTERN_BARS.length).toFixed(4) + "%";
    var to = (((i + 1) * 100) / PATTERN_BARS.length).toFixed(4) + "%";
    stops.push(PATTERN_BARS[i] + " " + from, PATTERN_BARS[i] + " " + to);
  }
  pattern.style.cssText =
    "position:absolute;left:0;top:0;width:100%;height:100%;display:none;" +
    "background-image:linear-gradient(to right, " + stops.join(", ") + ")";
  var label = document.createElement("div");
  label.style.cssText =
    "position:absolute;left:0;right:0;bottom:8%;padding:4px 8px;background:#000;color:#fff;" +
    "font:16px sans-serif;text-align:center;white-space:nowrap;overflow:hidden;text-overflow:ellipsis";
  pattern.appendChild(label);
  plane.appendChild(pattern);

  // The canvas background, redrawn over the video while it is translucent.
  var tint = document.createElement("div");
  tint.style.cssText = "position:absolute;left:0;top:0;width:100%;height:100%;display:none";

  var video: HTMLVideoElement | null = null;
  var rect: DisplayRect | null = null;
  var objectFit = "fill";
  var checkTimer = 0;
  // The opaque canvas background last reported, so each is reported once.
  var reported = "";

  function attach() {
    var root = document.documentElement;
    if (root && plane.parentNode !== root) {
      root.appendChild(plane);
    }
  }

  function paint() {
    attach();
    if (!rect) {
      plane.style.display = "none";
      return;
    }
    var canvas = canvasBackground();
    if (canvas && canvas.opaque) {
      plane.style.display = "none";
      var key = canvas.element + " " + canvas.color;
      if (key !== reported) {
        reported = key;
        raiseDiagnostic(
          "video-plane-hidden",
          "The Video Plane is hidden: the <" + canvas.element + "> background " + canvas.color +
            " is opaque and fills the screen" +
            (canvas.element === "body" ? " (a body background with none on html paints the whole canvas)" : "") +
            ". On a TV the page covers AVPlay's video, so it isn't seen.",
        );
      }
      return;
    }
    reported = "";
    tint.style.display = canvas ? "block" : "none";
    if (canvas) {
      tint.style.backgroundColor = canvas.color;
    }
    // App CSS px per TV pixel: 1 at 1920x1080, 2/3 at 1280x720.
    var scale = config.screen.width / 1920;
    plane.style.left = rect.x * scale + "px";
    plane.style.top = rect.y * scale + "px";
    plane.style.width = rect.width * scale + "px";
    plane.style.height = rect.height * scale + "px";
    plane.style.display = "block";
  }

  function watch(on: boolean) {
    if (on && !checkTimer) {
      checkTimer = setInterval(paint, CANVAS_CHECK_MS);
    } else if (!on && checkTimer) {
      clearInterval(checkTimer);
      checkTimer = 0;
    }
  }

  function unload() {
    if (video) {
      video.pause();
      video.removeAttribute("src");
      try {
        video.load();
      } catch (e) {
        // Old Engines may refuse to load nothing; the element is dropped anyway.
      }
      if (video.parentNode) {
        video.parentNode.removeChild(video);
      }
      video = null;
    }
    pattern.style.display = "none";
  }

  plane.appendChild(tint);

  return {
    load: function (url, decodable, settled) {
      unload();
      var element = (video = document.createElement("video"));
      element.style.cssText = "position:absolute;left:0;top:0;width:100%;height:100%;display:none";
      element.style.setProperty("object-fit", objectFit);
      element.preload = "auto";
      plane.insertBefore(element, tint);
      var done = false;
      function settle() {
        if (!done) {
          done = true;
          settled();
        }
      }
      var probe = setTimeout(function () {
        if (video === element && element.style.display === "none") {
          pattern.style.display = "block";
        }
        settle();
      }, DECODE_PROBE_MS);
      element.addEventListener("canplay", function () {
        if (video !== element || element.style.display === "block") {
          return;
        }
        clearTimeout(probe);
        pattern.style.display = "none";
        element.style.display = "block";
        decodable(element);
        settle();
      });
      element.addEventListener("error", function () {
        if (video !== element) {
          return;
        }
        clearTimeout(probe);
        reportMediaError(element);
        element.style.display = "none";
        pattern.style.display = "block";
        settle();
      });
      element.src = url;
    },

    unload: unload,

    show: function (next) {
      rect = next;
      paint();
      watch(!!next);
    },

    fit: function (next) {
      objectFit = next;
      if (video) {
        video.style.setProperty("object-fit", next);
      }
    },

    caption: function (text) {
      label.textContent = text;
    },
  };
}

/**
 * The root (canvas) background: the html element's, or the body's when html
 * has none, since CSS propagates it to the canvas then. Null while it is
 * fully transparent.
 */
function canvasBackground(): { element: string; color: string; opaque: boolean } | null {
  var html = getComputedStyle(document.documentElement);
  var element = "html";
  var color = html.backgroundColor;
  if (alpha(color) === 0 && html.backgroundImage === "none" && document.body) {
    element = "body";
    color = getComputedStyle(document.body).backgroundColor;
  }
  var opacity = alpha(color);
  return opacity === 0 ? null : { element: element, color: color, opaque: opacity >= 1 };
}

/**
 * The alpha of a computed colour: the fourth value of `rgba(...)`, or what
 * follows `/` in newer syntaxes (`oklch(0.5 0.1 120 / 0.5)`), as a number or
 * a percentage; colours without one are opaque.
 */
function alpha(color: string): number {
  if (!color || color === "transparent") {
    return 0;
  }
  var given = /^rgba\(.*,\s*([\d.]+)\)$/.exec(color) || /\/\s*([\d.]+%?)\s*\)$/.exec(color);
  if (!given) {
    return 1;
  }
  var value = parseFloat(given[1]);
  return given[1].charAt(given[1].length - 1) === "%" ? value / 100 : value;
}
