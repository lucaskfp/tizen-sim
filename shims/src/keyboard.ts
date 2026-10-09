// The desktop keyboard as a TV remote.
//
// Keys in the key map (Backspace, F1, ...) have no meaning on a TV: they are
// cancelled before the App sees them and the Host delivers the mapped Remote
// Key instead. Keys that already produce a Remote Key's keyCode (digits,
// Minus) pass through, unless the App hasn't registered them, as on a TV.
// Typing into a text field is left alone.
//
// Installed on window in the capture phase before any App script, so it runs
// before every App key listener.

var LONG_PRESS_MS = 1000;

// Holding Back makes the platform terminate the App; the App never sees a key
// (terminating-applications guide: "Long-pressing the Return/Exit key forces
// the application to terminate").
var TERMINATE = "";

// What holding a Remote Key does instead of pressing it (remote control
// guide, tables 1 and 2).
var LONG_PRESS: { [name: string]: string } = {
  Back: TERMINATE,
  MediaRewind: "MediaTrackPrevious",
  MediaFastForward: "MediaTrackNext",
};

var held = {
  keyCode: -1,
  timer: 0,
  sentLongPress: false,
};

function pressRemoteKey(name: string) {
  bridge.notify("keys.press", { key: name });
}

function swallow(e: KeyboardEvent) {
  e.preventDefault();
  e.stopImmediatePropagation();
}

function isTextField(target: EventTarget | null): boolean {
  var element = target as HTMLElement | null;
  if (!element || !element.tagName) {
    return false;
  }
  var tag = element.tagName.toLowerCase();
  return tag === "input" || tag === "textarea" || element.isContentEditable;
}

function onMappedKeyDown(e: KeyboardEvent, mapped: string) {
  if (!LONG_PRESS.hasOwnProperty(mapped)) {
    if (!e.repeat) {
      pressRemoteKey(mapped);
    }
    return;
  }
  // Sent on release; held long enough, the long-press action happens instead.
  var longPress = LONG_PRESS[mapped];
  if (held.keyCode === -1) {
    held.keyCode = e.keyCode;
    held.sentLongPress = false;
    held.timer = window.setTimeout(function () {
      held.sentLongPress = true;
      if (longPress === TERMINATE) {
        bridge.notify("application.terminate");
      } else {
        pressRemoteKey(longPress);
      }
    }, LONG_PRESS_MS);
  }
}

function onKeyDown(e: KeyboardEvent) {
  // Synthetic events don't come from the keyboard; leave them alone.
  if (e.isTrusted === false) {
    return;
  }
  var mapped = config.keyMap[String(e.keyCode)];
  if (mapped) {
    swallow(e);
    onMappedKeyDown(e, mapped);
    return;
  }
  var key = unregisteredRemoteKey(e);
  if (!key) {
    return;
  }
  // A held key is reported once, not for every auto-repeat.
  if (!e.repeat) {
    var reason = key.name + " is not registered (call tizen.tvinputdevice.registerKey)";
    if (config.lenient) {
      raiseDiagnostic("key-not-registered", reason + "; delivered anyway in Lenient Mode, a TV would drop it");
    } else {
      bridge.notify("keys.dropped", { key: key.name, reason: reason });
    }
  }
  if (!config.lenient) {
    swallow(e);
  }
}

/**
 * The Registrable Key this keyboard event produces, if the App hasn't
 * registered it. Such a key never reaches the App: no keydown, no auto-repeat,
 * no keyup (TD guides/text-input/input-device.md).
 */
function unregisteredRemoteKey(e: KeyboardEvent): RemoteKeyConfig | null {
  var key = findRemoteKeyByCode(e.keyCode);
  if (!key || key.mandatory || isKeyRegistered(key.name) || isTextField(e.target)) {
    return null;
  }
  return key;
}

function onKeyUp(e: KeyboardEvent) {
  if (e.isTrusted === false) {
    return;
  }
  var mapped = config.keyMap[String(e.keyCode)];
  if (!mapped) {
    if (unregisteredRemoteKey(e) && !config.lenient) {
      swallow(e);
    }
    return;
  }
  swallow(e);
  if (e.keyCode === held.keyCode) {
    window.clearTimeout(held.timer);
    if (!held.sentLongPress) {
      pressRemoteKey(mapped);
    }
    held.keyCode = -1;
  }
}

window.addEventListener("keydown", onKeyDown, true);
window.addEventListener("keyup", onKeyUp, true);
