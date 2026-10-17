// The App's launch screen (TD reference/config-xml.md, "Tizen launch screen"),
// shown over the page from launch until `ready_when`:
// - `first-paint`: the "frame,rendered" callback, taken here as the page's
//   first frame (the second animation frame after this script runs);
// - `complete`: the "load,complete" callback, taken here as the load event;
// - `custom`: the App calling `window.screen.show()`.
//
// Unverified, as the docs don't say:
// - how a TV lays out `background_image`, `image` and `image_border`. Here the
//   background image is stretched over the screen, the image is centred at
//   its own size, and `image_border` is ignored;
// - whether a TV has `screen.show()` for an App without a launch screen.
//   Here every App has it;
// - whether `screen.show()` also hides it before `first-paint` or `complete`
//   would. Here it does;
// - whether it shows again when the App goes to another page. Here it shows
//   once per launch. The Host counts its reloads and restarts as launches
//   and calls `config.forgetLaunchScreen` before them (see session.rs).

/** The sessionStorage mark that the launch screen was hidden. */
var HIDDEN_KEY = "__tizen-sim:launch-screen-hidden";

config.forgetLaunchScreen = function () {
  try {
    sessionStorage.removeItem(HIDDEN_KEY);
  } catch (e) {
    // Storage may be off; nothing was marked then.
  }
};

(function () {
  var launchScreen = config.launchScreen;
  var overlay: HTMLElement | null = null;

  function hiddenBefore(): boolean {
    try {
      return sessionStorage.getItem(HIDDEN_KEY) !== null;
    } catch (e) {
      return false;
    }
  }

  function hide() {
    try {
      sessionStorage.setItem(HIDDEN_KEY, "1");
    } catch (e) {
      // Storage may be off; the page then shows it again if it navigates.
    }
    if (overlay && overlay.parentNode) {
      overlay.parentNode.removeChild(overlay);
    }
    overlay = null;
  }

  Object.defineProperty(window.screen, "show", {
    value: function show() {
      hide();
    },
    configurable: true,
    writable: true,
  });

  if (!launchScreen || window.top !== window || hiddenBefore()) {
    return;
  }

  // An element name no App style matches, styled `!important` so none wins.
  overlay = document.createElement("tizen-sim-launch-screen");
  overlay.setAttribute("data-tizen-sim", "launch-screen");
  var images: string[] = [];
  var positions: string[] = [];
  var sizes: string[] = [];
  if (launchScreen.image) {
    images.push('url("' + launchScreen.image + '")');
    positions.push("center");
    sizes.push("auto");
  }
  if (launchScreen.backgroundImage) {
    images.push('url("' + launchScreen.backgroundImage + '")');
    positions.push("0 0");
    sizes.push("100% 100%");
  }
  var style: { [property: string]: string } = {
    display: "block",
    position: "fixed",
    left: "0",
    top: "0",
    width: "100%",
    height: "100%",
    margin: "0",
    padding: "0",
    border: "0",
    "z-index": "2147483647",
    "background-color": launchScreen.backgroundColor || "transparent",
    "background-image": images.length ? images.join(", ") : "none",
    "background-position": positions.join(", ") || "0 0",
    "background-size": sizes.join(", ") || "auto",
    "background-repeat": "no-repeat",
  };
  for (var property in style) {
    overlay.style.setProperty(property, style[property], "important");
  }
  // This script runs first in <head>, before there is a <body>.
  document.documentElement.appendChild(overlay);

  if (launchScreen.readyWhen === "first-paint") {
    requestAnimationFrame(function () {
      requestAnimationFrame(hide);
    });
  } else if (launchScreen.readyWhen === "complete") {
    window.addEventListener("load", hide);
  }
})();
