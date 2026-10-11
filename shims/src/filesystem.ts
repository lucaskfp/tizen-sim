// tizen.filesystem on the App's Sandbox, which the Host keeps on the
// developer's disk (sandbox.rs). What both of the API's shapes share: virtual
// paths, privileges, text encodings, filters and storages. Source:
// filesystem.html in the 4.0 TV reference (before the 5.0 rework) and the
// 9.0 one (after it); filesystem-file.ts and filesystem-handle.ts hold each
// shape's own interfaces.

var FS_READ_PRIVILEGE = "http://tizen.org/privilege/filesystem.read";
var FS_WRITE_PRIVILEGE = "http://tizen.org/privilege/filesystem.write";
var MEDIA_STORAGE_PRIVILEGE = "http://tizen.org/privilege/mediastorage";

// The virtual roots a TV has; `ringtones` isn't supported on TV (TD
// guides/data/file-system.md). Mirrors sandbox.rs.
var VIRTUAL_ROOTS = [
  "documents",
  "downloads",
  "images",
  "music",
  "videos",
  "camera",
  "wgt-package",
  "wgt-private",
  "wgt-private-tmp",
];

// The roots on the internal media storage, which from 5.0 also need the
// mediastorage privilege (the 5.0 reference's opening remark; the 4.0 one
// has no such remark).
var MEDIA_ROOTS = ["documents", "downloads", "images", "music", "videos", "camera"];

var PACKAGE_ROOT = "wgt-package";

interface VirtualPath {
  root: string;
  // The components below the root.
  parts: string[];
  // `<root>/<parts...>`, without trailing or doubled slashes.
  full: string;
}

interface Stat {
  kind: string;
  size: number;
  entries: number | null;
  created: number | null;
  modified: number | null;
  readOnly: boolean;
  // Only in listings.
  name?: string;
}

function contains(list: string[], value: string): boolean {
  for (var i = 0; i < list.length; i++) {
    if (list[i] === value) {
      return true;
    }
  }
  return false;
}

/** Splits a virtual path, or returns null for anything else. */
function parseVirtualPath(path: any): VirtualPath | null {
  if (typeof path !== "string") {
    return null;
  }
  var all = path.split("/");
  var parts: string[] = [];
  for (var i = 0; i < all.length; i++) {
    if (all[i] === "." || all[i] === "..") {
      return null;
    }
    if (all[i] !== "") {
      parts.push(all[i]);
    }
  }
  // An absolute path or file URI starts outside the roots.
  if (path.charAt(0) === "/" || !parts.length || !contains(VIRTUAL_ROOTS, parts[0])) {
    return null;
  }
  return { root: parts[0], parts: parts.slice(1), full: parts.join("/") };
}

/**
 * Parses `path` or throws `errorName`. A file URI or absolute path works on
 * a TV given the rights (filesystem.html: "file:///tmp" can be used as a
 * location), but the Sandbox has nothing outside the virtual roots, so it
 * also raises a Diagnostic.
 */
function requireVirtualPath(path: any, api: string, errorName: string): VirtualPath {
  var parsed = parseVirtualPath(path);
  if (parsed) {
    return parsed;
  }
  var text = String(path);
  if (text.charAt(0) === "/" || /^file:/i.test(text)) {
    raiseDiagnostic(
      "filesystem-outside-sandbox",
      api + " was given " + text + ", which is outside Tizen's virtual roots. A TV can reach such paths, " +
        "but the simulator keeps each App in its Sandbox, so here the call fails. Use a virtual root such as " +
        "wgt-private instead.",
    );
  }
  throw new WebAPIException(errorName, text + " is not a valid path");
}

/** The path one level up, or null for a virtual root. */
function parentPath(path: VirtualPath): VirtualPath | null {
  if (!path.parts.length) {
    return null;
  }
  return parseVirtualPath(path.full.slice(0, path.full.lastIndexOf("/")));
}

function childPath(path: VirtualPath, relative: string, errorName: string): VirtualPath {
  return requireVirtualPath(path.full + "/" + relative, "tizen.filesystem", errorName);
}

/** Checks the privilege every storage access needs besides the method's own. */
function requireStorageAccess(path: VirtualPath, api: string) {
  if (contains(MEDIA_ROOTS, path.root) && parseFloat(config.profile.version) >= 5) {
    requirePrivilege(MEDIA_STORAGE_PRIVILEGE, api);
  }
}

function sandboxCall(method: string, params: any): any {
  return bridge.call("filesystem." + method, params);
}

/** What is at `path`, or null if nothing is. */
function statOrNull(path: VirtualPath): Stat | null {
  try {
    return sandboxCall("stat", { path: path.full });
  } catch (e) {
    if (e instanceof WebAPIException && e.name === "NotFoundError") {
      return null;
    }
    throw e;
  }
}

/**
 * The URL the page can load the file at `path` from. On a TV it's a file://
 * URI, which a page served over HTTP can't load, so the simulator hands out
 * its dev server's URL for the file instead, with the Session's token,
 * which only the page has.
 */
function fileUrl(path: VirtualPath): string {
  var encoded: string[] = [];
  for (var i = 0; i < path.parts.length; i++) {
    encoded.push(encodeURIComponent(path.parts[i]));
  }
  var origin = location.protocol + "//" + location.host;
  if (path.root === PACKAGE_ROOT) {
    return origin + "/" + encoded.join("/");
  }
  var token = config.bridgeUrl.slice(config.bridgeUrl.indexOf("?"));
  return origin + config.sandboxPath + "/" + path.root + (encoded.length ? "/" + encoded.join("/") : "") + token;
}

/**
 * Runs `work` later, as Tizen's asynchronous methods do, passing its result
 * to `succeed` or its WebAPIException to `onerror` as a WebAPIError.
 */
function runLater(work: () => any, succeed: (result: any) => void, onerror?: any) {
  setTimeout(function () {
    var result: any;
    try {
      result = work();
    } catch (e) {
      if (!(e instanceof WebAPIException)) {
        throw e;
      }
      if (typeof onerror === "function") {
        onerror(new WebAPIError(e.name, e.message));
      }
      return;
    }
    succeed(result);
  }, 0);
}

function callIfFunction(callback: any, args: any[]) {
  if (typeof callback === "function") {
    callback.apply(null, args);
  }
}

// Text encodings: at least UTF-8 and ISO-8859-1 must be supported
// (File.openStream, FileHandle.readString); UTF-8 is the default.

function encodingName(encoding: any): string | null {
  if (encoding === undefined || encoding === null) {
    return "UTF-8";
  }
  var name = String(encoding).toUpperCase();
  if (name === "UTF-8" || name === "UTF8") {
    return "UTF-8";
  }
  if (name === "ISO-8859-1" || name === "LATIN1") {
    return "ISO-8859-1";
  }
  return null;
}

function encodeText(text: string, encoding: string): number[] {
  var bytes: number[] = [];
  for (var i = 0; i < text.length; i++) {
    var code = text.charCodeAt(i);
    if (encoding === "ISO-8859-1") {
      bytes.push(code < 256 ? code : 63);
      continue;
    }
    // A surrogate pair is one code point.
    if (code >= 0xd800 && code < 0xdc00 && i + 1 < text.length) {
      var low = text.charCodeAt(i + 1);
      if (low >= 0xdc00 && low < 0xe000) {
        code = 0x10000 + ((code - 0xd800) << 10) + (low - 0xdc00);
        i++;
      }
    }
    if (code < 0x80) {
      bytes.push(code);
    } else if (code < 0x800) {
      bytes.push(0xc0 | (code >> 6), 0x80 | (code & 63));
    } else if (code < 0x10000) {
      bytes.push(0xe0 | (code >> 12), 0x80 | ((code >> 6) & 63), 0x80 | (code & 63));
    } else {
      bytes.push(0xf0 | (code >> 18), 0x80 | ((code >> 12) & 63), 0x80 | ((code >> 6) & 63), 0x80 | (code & 63));
    }
  }
  return bytes;
}

/**
 * Decodes up to `maxChars` characters (code points; all if null) and says
 * how many bytes they took. An incomplete sequence at the end is left for
 * the next read; an invalid byte becomes U+FFFD.
 */
function decodeText(bytes: ArrayLike<number>, encoding: string, maxChars: number | null) {
  var text = "";
  var chars = 0;
  var i = 0;
  while (i < bytes.length && (maxChars === null || chars < maxChars)) {
    var first = bytes[i];
    if (encoding === "ISO-8859-1" || first < 0x80) {
      text += String.fromCharCode(first);
      i++;
      chars++;
      continue;
    }
    var length = first >= 0xf0 ? 4 : first >= 0xe0 ? 3 : first >= 0xc0 ? 2 : 0;
    if (length && i + length > bytes.length) {
      break;
    }
    var code = length === 4 ? first & 7 : length === 3 ? first & 15 : first & 31;
    for (var k = 1; k < length; k++) {
      if ((bytes[i + k] & 0xc0) !== 0x80) {
        length = 0;
        break;
      }
      code = (code << 6) | (bytes[i + k] & 63);
    }
    if (!length) {
      text += "�";
      i++;
    } else {
      if (code >= 0x10000) {
        code -= 0x10000;
        text += String.fromCharCode(0xd800 + (code >> 10), 0xdc00 + (code & 1023));
      } else {
        text += String.fromCharCode(code);
      }
      i += length;
    }
    chars++;
  }
  return { text: text, bytes: i };
}

function bytesToBase64(bytes: ArrayLike<number>): string {
  var binary = "";
  for (var i = 0; i < bytes.length; i++) {
    binary += String.fromCharCode(bytes[i] & 255);
  }
  return btoa(binary);
}

function base64ToBytes(base64: string): Uint8Array {
  var binary = atob(base64);
  var bytes = new Uint8Array(binary.length);
  for (var i = 0; i < binary.length; i++) {
    bytes[i] = binary.charCodeAt(i);
  }
  return bytes;
}

function readBytes(path: VirtualPath, offset: number, length: number | null): Uint8Array {
  var params: any = { path: path.full, offset: offset };
  if (length !== null) {
    params.length = length;
  }
  return base64ToBytes(sandboxCall("read", params).data);
}

/** Writes at `offset` and returns the file's new size. */
function writeBytes(path: VirtualPath, offset: number, bytes: ArrayLike<number>): number {
  return sandboxCall("write", { path: path.full, offset: offset, data: bytesToBase64(bytes) }).size;
}

// FileFilter (both references): names match exactly or with `%` wildcards,
// ignoring case, `\%` being a literal percent sign; dates match inclusively.

function nameMatches(name: string, pattern: string): boolean {
  var source = "";
  for (var i = 0; i < pattern.length; i++) {
    var c = pattern.charAt(i);
    if (c === "\\" && pattern.charAt(i + 1) === "%") {
      source += "%";
      i++;
    } else if (c === "%") {
      source += ".*";
    } else {
      source += c.replace(/[.*+?^${}()|[\]\\\/]/g, "\\$&");
    }
  }
  return new RegExp("^" + source + "$", "i").test(name);
}

function dateMatches(time: number | null, start: any, end: any): boolean {
  if (start instanceof Date && (time === null || time < start.getTime())) {
    return false;
  }
  return !(end instanceof Date && (time === null || time > end.getTime()));
}

function filterMatches(entry: Stat, filter: any): boolean {
  if (!filter) {
    return true;
  }
  if (filter.name !== undefined && filter.name !== null && !nameMatches(entry.name || "", String(filter.name))) {
    return false;
  }
  if (typeof filter.isFile === "boolean" && filter.isFile !== (entry.kind === "file")) {
    return false;
  }
  if (typeof filter.isDirectory === "boolean" && filter.isDirectory !== (entry.kind === "directory")) {
    return false;
  }
  return (
    dateMatches(entry.modified, filter.startModified, filter.endModified) &&
    dateMatches(entry.created, filter.startCreated, filter.endCreated)
  );
}

/** A directory's entries that pass `filter`. */
function listDirectoryEntries(path: VirtualPath, filter: any): Stat[] {
  var entries: Stat[] = sandboxCall("list", { path: path.full });
  var matching: Stat[] = [];
  for (var i = 0; i < entries.length; i++) {
    if (filterMatches(entries[i], filter)) {
      matching.push(entries[i]);
    }
  }
  return matching;
}

// Storages: the Sandbox's virtual roots, all internal and mounted. Which
// labels a TV lists is unverified: listStorages says only that labels "can
// differ depending on platform implementation".

var nextStorageListenerId = 1;

function storages() {
  var list: any[] = [];
  for (var i = 0; i < VIRTUAL_ROOTS.length; i++) {
    list.push({ label: VIRTUAL_ROOTS[i], type: "INTERNAL", state: "MOUNTED" });
  }
  return list;
}

var storageMethods = {
  // The maximum path length; the reference's example gives 4096.
  maxPathLength: 4096,

  getStorage: function (label: string, onsuccess: (storage: any) => void, onerror?: any) {
    requirePrivilege(FS_READ_PRIVILEGE, "tizen.filesystem.getStorage");
    requireFunction(onsuccess, "onsuccess");
    requireOptionalFunction(onerror, "onerror");
    runLater(
      function () {
        var all = storages();
        for (var i = 0; i < all.length; i++) {
          if (all[i].label === String(label)) {
            return all[i];
          }
        }
        throw new WebAPIException("NotFoundError", "No storage is labelled " + label);
      },
      onsuccess,
      onerror,
    );
  },

  listStorages: function (onsuccess: (storages: any[]) => void, onerror?: any) {
    requirePrivilege(FS_READ_PRIVILEGE, "tizen.filesystem.listStorages");
    requireFunction(onsuccess, "onsuccess");
    requireOptionalFunction(onerror, "onerror");
    runLater(storages, onsuccess, onerror);
  },

  // The Sandbox has no external storage, so storage states never change.
  addStorageStateChangeListener: function (onsuccess: (storage: any) => void, onerror?: any) {
    requirePrivilege(FS_WRITE_PRIVILEGE, "tizen.filesystem.addStorageStateChangeListener");
    requireFunction(onsuccess, "onsuccess");
    requireOptionalFunction(onerror, "onerror");
    return nextStorageListenerId++;
  },

  removeStorageStateChangeListener: function (watchId: number) {
    requirePrivilege(FS_WRITE_PRIVILEGE, "tizen.filesystem.removeStorageStateChangeListener");
  },
};

/** tizen.filesystem with the members the Version Profile has. */
function createFilesystem() {
  var filesystem: any = {};
  var groups = [storageMethods, fileApiMethods, pathApiMethods];
  for (var g = 0; g < groups.length; g++) {
    var methods: any = groups[g];
    for (var name in methods) {
      if (methods.hasOwnProperty(name) && available("tizen.filesystem." + name)) {
        filesystem[name] = methods[name];
      }
    }
  }
  return filesystem;
}
