// The App's launch screen (TD reference/config-xml.md, "Tizen launch screen"),
// shown over the page from launch until `ready_when`:
// - `first-paint`: the "frame,rendered" callback, taken here as the page's
//   first frame (the second animation frame after this script runs);
// - `complete`: the "load,complete" callback, taken here as the load event;
// - `custom`: the App calling `window.screen.show()`.
//
// Measured on a 2019 TV (Tizen 5.0, #113), where the docs say nothing:
// - every App has `screen.show()`, with or without a launch screen, and it
//   hides the launch screen only with `custom`: under `complete` an earlier
//   call leaves it up until the load event;
// - `first-paint` hides it before the load event, `complete` waits for it;
// - it stays up when the App goes to another page before hiding it, until
//   that page calls `show()`; once hidden, it doesn't show again. Here the
//   Host counts its reloads and restarts as launches and calls
//   `config.forgetLaunchScreen` before them (see session.rs);
// - the background image is stretched over the screen, and the image is
//   scaled to fit the screen, keeping its proportions, and centred.
//   `image_border` changed nothing visible: "0px stretch", "40px stretch",
//   "40px repeat" and "40px round" all drew the image the same, so it is
//   ignored here. The docs don't list its values; others are unverified.

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
      if (launchScreen && launchScreen.readyWhen === "custom") {
        hide();
      }
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
    sizes.push("contain");
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
