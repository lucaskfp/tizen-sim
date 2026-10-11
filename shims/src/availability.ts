// API availability: which Shimmed members the Version Profile has, from the
// table in crates/tizen-sim-core/data/apis.toml. Names are members' paths
// (`tizen.systeminfo.getDeviceUptime`) or `<Enum>.<VALUE>`.

function available(name: string): boolean {
  var absent = config.profile.apis.absent;
  for (var i = 0; i < absent.length; i++) {
    if (absent[i] === name) {
      return false;
    }
  }
  return true;
}

/**
 * Makes each deprecated method under `root` (`tizen` or `webapis`) log a
 * deprecation warning on every call. The policy names "DEPRECATION WARNING"
 * log messages (TD reference/deprecation-policy.md, a native example); the
 * exact wording a web App's console gets is unverified.
 */
function warnOnDeprecated(root: string) {
  var deprecated = config.profile.apis.deprecated;
  for (var i = 0; i < deprecated.length; i++) {
    var path = deprecated[i].split(".");
    if (path[0] !== root) {
      continue;
    }
    var owner: any = window;
    for (var j = 0; j < path.length - 1 && owner; j++) {
      owner = owner[path[j]];
    }
    var member = path[path.length - 1];
    if (owner && typeof owner[member] === "function") {
      owner[member] = deprecatedMethod(deprecated[i], owner[member]);
    }
  }
}

function deprecatedMethod(name: string, method: Function): Function {
  return function (this: any) {
    console.warn(
      "DEPRECATION WARNING: " + name + "() is deprecated and will be removed from a future release.",
    );
    return method.apply(this, arguments);
  };
}
