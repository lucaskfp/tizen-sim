// tizen.tvinputdevice: which Remote Keys the App has registered.
//
// Registration lives here in the page, and the Host asks through
// `window.__TIZEN_SIM__.keys.isRegistered` before delivering a key, so a
// registerKey() call takes effect immediately and resets on reload, as an App
// relaunch does on a TV.

var TV_INPUT_PRIVILEGE = "http://tizen.org/privilege/tv.inputdevice";

var registeredKeys: { [name: string]: boolean } = {};

function findRemoteKey(name: string): RemoteKeyConfig | null {
  var keys = config.profile.remoteKeys;
  for (var i = 0; i < keys.length; i++) {
    if (keys[i].name === name) {
      return keys[i];
    }
  }
  return null;
}

function findRemoteKeyByCode(code: number): RemoteKeyConfig | null {
  var keys = config.profile.remoteKeys;
  for (var i = 0; i < keys.length; i++) {
    if (keys[i].code === code) {
      return keys[i];
    }
  }
  return null;
}

function isKeyRegistered(name: string): boolean {
  return registeredKeys.hasOwnProperty(name);
}

/** Throws InvalidValuesError unless `name` is a Registrable Key of these TVs. */
function requireRegistrable(name: string) {
  var key = findRemoteKey(name);
  if (!key) {
    throw new WebAPIException("InvalidValuesError", "Unsupported key: " + name);
  }
  if (key.mandatory) {
    throw new WebAPIException("InvalidValuesError", name + " is a Mandatory Key and can't be registered");
  }
}

/** Exit is the Back key's long press, which Samsung says Apps must not register. */
function warnIfExit(name: string) {
  if (name === "Exit") {
    raiseDiagnostic(
      "exit-key-registered",
      "Registering Exit captures the Back key's long press, which on a TV terminates the App; " +
        "Samsung's guidelines say: do not register the Return/Exit key long-press.",
    );
  }
}

function keyInfo(key: RemoteKeyConfig) {
  return { name: key.name, code: key.code };
}

function changeRegistration(
  api: string,
  names: string[],
  register: boolean,
  successCallback?: () => void,
  errorCallback?: (e: WebAPIException) => void,
) {
  requirePrivilege(TV_INPUT_PRIVILEGE, api);
  try {
    for (var i = 0; i < names.length; i++) {
      requireRegistrable(names[i]);
    }
  } catch (e) {
    var error = e as WebAPIException;
    failLater(errorCallback, error.name, error.message);
    return;
  }
  for (var j = 0; j < names.length; j++) {
    if (register) {
      warnIfExit(names[j]);
      registeredKeys[names[j]] = true;
    } else {
      delete registeredKeys[names[j]];
    }
  }
  if (typeof successCallback === "function") {
    setTimeout(successCallback, 0);
  }
}

var tvinputdevice = {
  getSupportedKeys: function () {
    requirePrivilege(TV_INPUT_PRIVILEGE, "tizen.tvinputdevice.getSupportedKeys");
    var supported = [];
    var keys = config.profile.remoteKeys;
    for (var i = 0; i < keys.length; i++) {
      if (!keys[i].mandatory) {
        supported.push(keyInfo(keys[i]));
      }
    }
    return supported;
  },

  getKey: function (name: string) {
    requirePrivilege(TV_INPUT_PRIVILEGE, "tizen.tvinputdevice.getKey");
    var key = findRemoteKey(name);
    return key && !key.mandatory ? keyInfo(key) : null;
  },

  registerKey: function (name: string) {
    requirePrivilege(TV_INPUT_PRIVILEGE, "tizen.tvinputdevice.registerKey");
    requireRegistrable(name);
    warnIfExit(name);
    registeredKeys[name] = true;
  },

  unregisterKey: function (name: string) {
    requirePrivilege(TV_INPUT_PRIVILEGE, "tizen.tvinputdevice.unregisterKey");
    requireRegistrable(name);
    delete registeredKeys[name];
  },

  registerKeyBatch: function (
    names: string[],
    successCallback?: () => void,
    errorCallback?: (e: WebAPIException) => void,
  ) {
    changeRegistration("tizen.tvinputdevice.registerKeyBatch", names, true, successCallback, errorCallback);
  },

  unregisterKeyBatch: function (
    names: string[],
    successCallback?: () => void,
    errorCallback?: (e: WebAPIException) => void,
  ) {
    changeRegistration("tizen.tvinputdevice.unregisterKeyBatch", names, false, successCallback, errorCallback);
  },
};

config.keys = { isRegistered: isKeyRegistered };
