// tizen.filesystem since the 5.0 rework: path-based methods, and openFile()
// handing out FileHandles. Source: filesystem.html in the 9.0 TV reference.
// Absent before 5.0 (apis.toml).

var OPEN_MODES = ["a", "r", "rw", "rwo", "w"];
var SEEK_POSITIONS = ["BEGIN", "CURRENT", "END"];

// The maximum name length; the reference's example gives 255.
var MAX_NAME_LENGTH = 255;

/**
 * A FileHandle on the file at `path`, opened in `mode`. Reads and writes
 * happen at `position`. The NonBlocking methods queue their work and run it
 * in order, later; close() first finishes what is queued (#FileHandle::close).
 */
function fileHandle(givenPath: string, path: VirtualPath, mode: string): any {
  var position = 0;
  var closed = false;
  var queue: Array<() => void> = [];
  var canRead = mode === "r" || mode === "rw" || mode === "rwo";
  var canWrite = mode !== "r";

  function size(): number {
    return sandboxCall("stat", { path: path.full }).size;
  }

  function requireOpen() {
    if (closed) {
      throw new WebAPIException("IOError", "The FileHandle is closed");
    }
  }

  function requireAccess(write: boolean) {
    requireOpen();
    if (write ? !canWrite : !canRead) {
      throw new WebAPIException(
        "IOError",
        "The file was opened in mode " + mode + ", which doesn't allow " + (write ? "writing" : "reading"),
      );
    }
  }

  function requireEncoding(encoding: any): string {
    var name = encodingName(encoding);
    if (!name) {
      throw new WebAPIException("NotSupportedError", "Unsupported encoding: " + encoding);
    }
    return name;
  }

  /** The number of bytes to read: all that is left when absent or null. */
  function requireSize(value: any): number | null {
    if (value === undefined || value === null) {
      return null;
    }
    var n = Number(value);
    if (isNaN(n)) {
      throw new WebAPIException("TypeMismatchError", "The size must be a number");
    }
    if (n < 0) {
      throw new WebAPIException("InvalidValuesError", "The size must not be negative");
    }
    return Math.floor(n);
  }

  function read(length: number | null): Uint8Array {
    requireAccess(false);
    var bytes = readBytes(path, position, length);
    position += bytes.length;
    return bytes;
  }

  /** Writes at the position; in mode a, always at the end. */
  function write(bytes: ArrayLike<number>): number {
    requireAccess(true);
    if (mode === "a") {
      position = size();
    }
    writeBytes(path, position, bytes);
    position += bytes.length;
    return bytes.length;
  }

  function seek(offset: any, whence: any): number {
    var from = whence === undefined ? "BEGIN" : whence;
    if (!contains(SEEK_POSITIONS, from) || isNaN(Number(offset))) {
      throw new WebAPIException("TypeMismatchError", "seek takes an offset and BEGIN, CURRENT or END");
    }
    requireOpen();
    // In mode a writes still go to the end (see `write`), though seek moves
    // the position (measured on a Tizen 5.0 TV).
    var base = from === "BEGIN" ? 0 : from === "CURRENT" ? position : size();
    var target = base + Math.floor(Number(offset));
    if (target < 0) {
      throw new WebAPIException("IOError", "Can't seek before the start of the file");
    }
    position = target;
    return position;
  }

  function readString(count: any, encoding: string): string {
    requireAccess(false);
    var chars = requireSize(count);
    // A character takes at most 4 bytes in UTF-8.
    var length = chars === null ? null : encoding === "UTF-8" ? chars * 4 : chars;
    var decoded = decodeText(readBytes(path, position, length), encoding, chars);
    position += decoded.bytes;
    return decoded.text;
  }

  function writeBlob(blob: any) {
    if (!(blob instanceof Blob)) {
      throw new WebAPIException("TypeMismatchError", "blob must be a Blob");
    }
    write(blobBytes(blob));
  }

  function writeData(data: any) {
    if (!(data instanceof Uint8Array)) {
      throw new WebAPIException("TypeMismatchError", "data must be a Uint8Array");
    }
    write(data);
  }

  function close() {
    drain();
    requireOpen();
    closed = true;
  }

  /** Runs the queued work now, in order. */
  function drain() {
    while (queue.length) {
      queue.shift()!();
    }
  }

  /** Queues `work`, reporting its result to `onsuccess` or its error to `onerror`. */
  function later(work: () => any, onsuccess: any, onerror: any) {
    requireOptionalFunction(onsuccess, "onsuccess");
    requireOptionalFunction(onerror, "onerror");
    queue.push(function () {
      var result: any;
      try {
        result = work();
      } catch (e) {
        if (!(e instanceof WebAPIException)) {
          throw e;
        }
        callIfFunction(onerror, [new WebAPIError(e.name, e.message)]);
        return;
      }
      callIfFunction(onsuccess, result === undefined ? [] : [result]);
    });
    setTimeout(function () {
      if (queue.length) {
        queue.shift()!();
      }
    }, 0);
  }

  return {
    path: givenPath,

    seek: seek,
    seekNonBlocking: function (offset: any, onsuccess?: any, onerror?: any, whence?: any) {
      if (whence !== undefined && !contains(SEEK_POSITIONS, whence)) {
        throw new WebAPIException("TypeMismatchError", "whence must be BEGIN, CURRENT or END");
      }
      later(function () {
        return seek(offset, whence);
      }, onsuccess, onerror);
    },

    readString: function (count?: any, inputEncoding?: any) {
      return readString(count, requireEncoding(inputEncoding));
    },
    readStringNonBlocking: function (onsuccess?: any, onerror?: any, count?: any, inputEncoding?: any) {
      var encoding = requireEncoding(inputEncoding);
      requireSize(count);
      later(function () {
        return readString(count, encoding);
      }, onsuccess, onerror);
    },

    writeString: function (inputString: any, outputEncoding?: any) {
      return write(encodeText(String(inputString), requireEncoding(outputEncoding)));
    },
    writeStringNonBlocking: function (inputString: any, onsuccess?: any, onerror?: any, outputEncoding?: any) {
      var bytes = encodeText(String(inputString), requireEncoding(outputEncoding));
      later(function () {
        return write(bytes);
      }, onsuccess, onerror);
    },

    readBlob: function (size?: any) {
      return new Blob([read(requireSize(size))]);
    },
    readBlobNonBlocking: function (onsuccess?: any, onerror?: any, size?: any) {
      var length = requireSize(size);
      later(function () {
        return new Blob([read(length)]);
      }, onsuccess, onerror);
    },

    writeBlob: writeBlob,
    writeBlobNonBlocking: function (blob: any, onsuccess?: any, onerror?: any) {
      if (!(blob instanceof Blob)) {
        throw new WebAPIException("TypeMismatchError", "blob must be a Blob");
      }
      later(function () {
        writeBlob(blob);
      }, onsuccess, onerror);
    },

    readData: function (size?: any) {
      return read(requireSize(size));
    },
    readDataNonBlocking: function (onsuccess?: any, onerror?: any, size?: any) {
      var length = requireSize(size);
      later(function () {
        return read(length);
      }, onsuccess, onerror);
    },

    writeData: writeData,
    writeDataNonBlocking: function (data: any, onsuccess?: any, onerror?: any) {
      if (!(data instanceof Uint8Array)) {
        throw new WebAPIException("TypeMismatchError", "data must be a Uint8Array");
      }
      later(function () {
        writeData(data);
      }, onsuccess, onerror);
    },

    // Every write goes straight to the disk, so there is nothing to flush.
    flush: requireOpen,
    flushNonBlocking: function (onsuccess?: any, onerror?: any) {
      later(requireOpen, onsuccess, onerror);
    },
    sync: requireOpen,
    syncNonBlocking: function (onsuccess?: any, onerror?: any) {
      later(requireOpen, onsuccess, onerror);
    },

    close: close,
    closeNonBlocking: function (onsuccess?: any, onerror?: any) {
      later(close, onsuccess, onerror);
    },
  };
}

/**
 * A Blob's bytes, read synchronously as writeBlob() must. The page has no
 * FileReaderSync (workers only), so it's read with a blocking request for
 * its object URL.
 */
function blobBytes(blob: Blob): Uint8Array {
  var url = URL.createObjectURL(blob);
  try {
    var request = new XMLHttpRequest();
    request.open("GET", url, false);
    request.overrideMimeType("text/plain; charset=x-user-defined");
    request.send();
    var text = request.responseText;
    var bytes = new Uint8Array(text.length);
    for (var i = 0; i < text.length; i++) {
      bytes[i] = text.charCodeAt(i) & 255;
    }
    return bytes;
  } finally {
    URL.revokeObjectURL(url);
  }
}

/** Parses a path argument of a 5.0 method: InvalidValuesError if invalid. */
function requirePathArgument(path: any, api: string): VirtualPath {
  var parsed = requireVirtualPath(path, "tizen.filesystem." + api, "InvalidValuesError");
  requireStorageAccess(parsed, "tizen.filesystem." + api);
  return parsed;
}

function lastPart(path: VirtualPath): string {
  return path.parts.length ? path.parts[path.parts.length - 1] : path.root;
}

/**
 * A path-based change: privileges, then arguments (`prepare`) and callbacks
 * are checked now; the change `prepare` returns is made later, and
 * `onsuccess` gets the path it reports.
 */
function changeLater(
  privileges: string[],
  api: string,
  onsuccess: any,
  onerror: any,
  prepare: () => () => VirtualPath | null,
) {
  for (var i = 0; i < privileges.length; i++) {
    requirePrivilege(privileges[i], "tizen.filesystem." + api);
  }
  var change = prepare();
  requireOptionalFunction(onsuccess, "successCallback");
  requireOptionalFunction(onerror, "errorCallback");
  runLater(
    change,
    function (reported: VirtualPath | null) {
      callIfFunction(onsuccess, [reported ? reported.full : ""]);
    },
    onerror,
  );
}

var READ_WRITE = [FS_READ_PRIVILEGE, FS_WRITE_PRIVILEGE];

/** copyFile, copyDirectory, moveFile and moveDirectory. */
function transfer(api: string, call: string, kind: string, into: boolean) {
  return function (sourcePath: any, destinationPath: any, overwrite?: any, onsuccess?: any, onerror?: any) {
    changeLater(READ_WRITE, api, onsuccess, onerror, function () {
      var from = requirePathArgument(sourcePath, api);
      var destination = requirePathArgument(destinationPath, api);
      // moveFile and moveDirectory take the directory to move into.
      var to = into ? childPath(destination, lastPart(from), "InvalidValuesError") : destination;
      return function () {
        sandboxCall(call, { from: from.full, to: to.full, kind: kind, overwrite: !!overwrite });
        return to;
      };
    });
  };
}

var pathApiMethods = {
  maxNameLength: MAX_NAME_LENGTH,

  openFile: function (path: any, openMode: any, makeParents?: any) {
    if (!contains(OPEN_MODES, openMode)) {
      throw new WebAPIException("TypeMismatchError", "openMode must be one of a, r, rw, rwo, w");
    }
    // Writing needs filesystem.write (a, rw, rwo, w), reading filesystem.read
    // (r, rw, rwo) (#FileSystemManager::openFile).
    if (openMode !== "r") {
      requirePrivilege(FS_WRITE_PRIVILEGE, "tizen.filesystem.openFile");
    }
    if (openMode === "r" || openMode === "rw" || openMode === "rwo") {
      requirePrivilege(FS_READ_PRIVILEGE, "tizen.filesystem.openFile");
    }
    // The reference lists no InvalidValuesError: an unusable path is
    // NotFoundError, as on a Tizen 5.0 TV (measured).
    var parsed = requireVirtualPath(path, "tizen.filesystem.openFile", "NotFoundError");
    requireStorageAccess(parsed, "tizen.filesystem.openFile");
    if (openMode === "r" || openMode === "rw") {
      // A directory opens too; reading it then fails (measured).
      sandboxCall("stat", { path: parsed.full });
    } else {
      sandboxCall("createFile", {
        path: parsed.full,
        // true by default (#FileSystemManager::openFile).
        makeParents: makeParents === undefined || !!makeParents,
        ifExists: openMode === "a" ? "keep" : "truncate",
      });
    }
    return fileHandle(String(path), parsed, openMode);
  },

  createDirectory: function (path: any, makeParents?: any, successCallback?: any, errorCallback?: any) {
    changeLater([FS_WRITE_PRIVILEGE], "createDirectory", successCallback, errorCallback, function () {
      var parsed = requirePathArgument(path, "createDirectory");
      return function () {
        // true by default (#FileSystemManager::createDirectory).
        sandboxCall("createDirectory", { path: parsed.full, makeParents: makeParents === undefined || !!makeParents });
        return parsed;
      };
    });
  },

  // Deleting reports the parent's path (#PathSuccessCallback::onsuccess).
  deleteFile: function (path: any, successCallback?: any, errorCallback?: any) {
    changeLater([FS_WRITE_PRIVILEGE], "deleteFile", successCallback, errorCallback, function () {
      var parsed = requirePathArgument(path, "deleteFile");
      return function () {
        sandboxCall("delete", { path: parsed.full, kind: "file" });
        return parentPath(parsed);
      };
    });
  },

  deleteDirectory: function (path: any, recursive?: any, successCallback?: any, errorCallback?: any) {
    changeLater([FS_WRITE_PRIVILEGE], "deleteDirectory", successCallback, errorCallback, function () {
      var parsed = requirePathArgument(path, "deleteDirectory");
      return function () {
        // true by default (#FileSystemManager::deleteDirectory).
        sandboxCall("delete", { path: parsed.full, kind: "directory", recursive: recursive === undefined || !!recursive });
        return parentPath(parsed);
      };
    });
  },

  copyFile: transfer("copyFile", "copy", "file", false),
  copyDirectory: transfer("copyDirectory", "copy", "directory", false),
  moveFile: transfer("moveFile", "move", "file", true),
  moveDirectory: transfer("moveDirectory", "move", "directory", true),

  rename: function (path: any, newName: any, successCallback?: any, errorCallback?: any) {
    changeLater([FS_WRITE_PRIVILEGE], "rename", successCallback, errorCallback, function () {
      var from = requirePathArgument(path, "rename");
      var name = String(newName);
      var parent = parentPath(from);
      if (!parent || !name || name.indexOf("/") >= 0 || name === "." || name === "..") {
        throw new WebAPIException("InvalidValuesError", newName + " is not a valid new name");
      }
      var to = childPath(parent, name, "InvalidValuesError");
      return function () {
        sandboxCall("move", { from: from.full, to: to.full });
        return to;
      };
    });
  },

  listDirectory: function (path: any, successCallback: any, errorCallback?: any, filter?: any) {
    requirePrivilege(FS_READ_PRIVILEGE, "tizen.filesystem.listDirectory");
    requireFunction(successCallback, "successCallback");
    requireOptionalFunction(errorCallback, "errorCallback");
    var parsed = requirePathArgument(path, "listDirectory");
    runLater(
      function () {
        // A file gives IOError (measured on a Tizen 5.0 TV).
        var stat = sandboxCall("stat", { path: parsed.full });
        if (stat.kind !== "directory") {
          throw new WebAPIException("IOError", parsed.full + " is not a directory");
        }
        var entries = listDirectoryEntries(parsed, filter);
        var names: string[] = [];
        for (var i = 0; i < entries.length; i++) {
          names.push(entries[i].name || "");
        }
        return names;
      },
      function (names: string[]) {
        successCallback(names, String(path));
      },
      errorCallback,
    );
  },

  // Needs no privilege, and doesn't check that the path exists (#FileSystemManager::toURI).
  toURI: function (path: any) {
    return fileUrl(requireVirtualPath(path, "tizen.filesystem.toURI", "InvalidValuesError"));
  },

  isFile: function (path: any) {
    requirePrivilege(FS_READ_PRIVILEGE, "tizen.filesystem.isFile");
    return sandboxCall("stat", { path: requirePathArgument(path, "isFile").full }).kind === "file";
  },

  isDirectory: function (path: any) {
    requirePrivilege(FS_READ_PRIVILEGE, "tizen.filesystem.isDirectory");
    return sandboxCall("stat", { path: requirePathArgument(path, "isDirectory").full }).kind === "directory";
  },

  pathExists: function (path: any) {
    requirePrivilege(FS_READ_PRIVILEGE, "tizen.filesystem.pathExists");
    // A path with `.` or `..` exists nowhere (measured on a Tizen 5.0 TV).
    if (hasDotSegments(path)) {
      return false;
    }
    return statOrNull(requirePathArgument(path, "pathExists")) !== null;
  },

  // Strips trailing slashes, then drops the last component
  // (#FileSystemManager::getDirName). A path without "/" comes back as it
  // is, and "/" stays "/" (measured on a Tizen 5.0 TV).
  getDirName: function (path: any) {
    var given = String(path);
    var text = given.replace(/\/+$/, "");
    if (!text) {
      return given ? "/" : "";
    }
    var slash = text.lastIndexOf("/");
    if (slash < 0) {
      return text;
    }
    return text.slice(0, slash).replace(/\/+$/, "") || "/";
  },
};
