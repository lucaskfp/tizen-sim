// The desktop keyboard as a TV remote.
//
// Keys in the key map (Backspace, F1, ...) have no meaning on a TV: they are
// cancelled before the App sees them and the Host delivers the mapped Remote
// Key instead. Keys that already produce a Remote Key's keyCode (digits) pass
// through, unless the App hasn't registered them, as on a TV.
//
// Installed on window in the capture phase before any App script, so it runs
// before every App key listener.

var LONG_PRESS_MS = 1000;

var backHold = {
  keyCode: -1,
  timer: 0,
  sentExit: false,
};

function pressRemoteKey(name: string) {
  bridge.notify("keys.press", { key: name });
}

function swallow(e: KeyboardEvent) {
  e.preventDefault();
  e.stopImmediatePropagation();
}

function onKeyDown(e: KeyboardEvent) {
  // Synthetic events don't come from the keyboard; leave them alone.
  if (e.isTrusted === false) {
    return;
  }
  var mapped = config.keyMap[String(e.keyCode)];
  if (mapped) {
    swallow(e);
    if (mapped === "Back") {
      // Back on key release; held long enough, it's Exit instead.
      if (backHold.keyCode === -1) {
        backHold.keyCode = e.keyCode;
        backHold.sentExit = false;
        backHold.timer = window.setTimeout(function () {
          backHold.sentExit = true;
          pressRemoteKey("Exit");
        }, LONG_PRESS_MS);
      }
    } else if (!e.repeat) {
      pressRemoteKey(mapped);
    }
    return;
  }
  var key = findRemoteKeyByCode(e.keyCode);
  if (!key || key.mandatory || isKeyRegistered(key.name) || e.repeat) {
    return;
  }
  var reason = key.name + " is not registered (call tizen.tvinputdevice.registerKey)";
  if (config.lenient) {
    raiseDiagnostic("key-not-registered", reason + "; delivered anyway in Lenient Mode, a TV would drop it");
    return;
  }
  swallow(e);
  bridge.notify("keys.dropped", { key: key.name, reason: reason });
}

function onKeyUp(e: KeyboardEvent) {
  if (e.isTrusted === false) {
    return;
  }
  if (!config.keyMap[String(e.keyCode)]) {
    return;
  }
  swallow(e);
  if (e.keyCode === backHold.keyCode) {
    window.clearTimeout(backHold.timer);
    if (!backHold.sentExit) {
      pressRemoteKey("Back");
    }
    backHold.keyCode = -1;
  }
}

window.addEventListener("keydown", onKeyDown, true);
window.addEventListener("keyup", onKeyUp, true);
