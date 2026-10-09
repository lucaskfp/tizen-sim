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

// Remote Keys that send a different key when held (remote control guide, tables 1 and 2).
var LONG_PRESS: { [name: string]: string } = {
  Back: "Exit",
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
  var longPress = LONG_PRESS[mapped];
  if (!longPress) {
    if (!e.repeat) {
      pressRemoteKey(mapped);
    }
    return;
  }
  // Sent on release; held long enough, it's the long-press key instead.
  if (held.keyCode === -1) {
    held.keyCode = e.keyCode;
    held.sentLongPress = false;
    held.timer = window.setTimeout(function () {
      held.sentLongPress = true;
      pressRemoteKey(longPress);
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
  var key = findRemoteKeyByCode(e.keyCode);
  if (!key || key.mandatory || isKeyRegistered(key.name) || e.repeat || isTextField(e.target)) {
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
  var mapped = config.keyMap[String(e.keyCode)];
  if (!mapped) {
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
