// Web features the Version Profile's TVs lack, by Samsung's TV web engine
// specifications (TV specifications/web-engine-specifications.html), where
// the Engine has them. config.profile.webFeatures lists what to delete, from
// crates/tizen-sim-core/data/web_features.toml.
//
// Runs before any App script. Workers and documents tizen-sim doesn't serve
// (about:blank frames) keep the Engine's features.

/**
 * Deletes each path: a path from `window` whose last segment is deleted from
 * its owner and the owner's prototypes, or a global name prefix ending in `*`.
 * Members that aren't configurable (ES2015's Number constants) stay.
 */
function removeMissingFeatures() {
  var missing = config.profile.webFeatures;
  for (var i = 0; i < missing.length; i++) {
    for (var j = 0; j < missing[i].paths.length; j++) {
      removePath(missing[i].paths[j]);
    }
  }
}

function removePath(path: string) {
  var parts = path.split(".");
  var owner: any = window;
  for (var i = 0; i < parts.length - 1; i++) {
    owner = owner[parts[i]];
    if (owner == null) {
      return;
    }
  }
  var last = parts[parts.length - 1];
  if (last.charAt(last.length - 1) === "*") {
    var prefix = last.slice(0, -1);
    var names = Object.getOwnPropertyNames(owner);
    for (var k = 0; k < names.length; k++) {
      if (names[k].indexOf(prefix) === 0) {
        deleteMember(owner, names[k]);
      }
    }
    return;
  }
  for (var level = owner; level; level = Object.getPrototypeOf(level)) {
    deleteMember(level, last);
  }
}

function deleteMember(owner: any, name: string) {
  if (Object.prototype.hasOwnProperty.call(owner, name)) {
    try {
      delete owner[name];
    } catch (e) {
      // Not configurable: it stays.
    }
  }
}
