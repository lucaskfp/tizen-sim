// tizen.filesystem before the 5.0 rework: resolve() hands out File objects,
// and File.openStream() FileStreams. Source: filesystem.html in the 4.0 TV
// reference. Deprecated in 5.0 and gone from the 6.0 reference (apis.toml).

var FILE_MODES = ["a", "r", "rw", "w"];

function requireFileMode(mode: any): string {
  if (!contains(FILE_MODES, mode)) {
    throw new WebAPIException("TypeMismatchError", "mode must be one of a, r, rw, w");
  }
  return mode;
}

/**
 * A File for what `stat` describes at `path`. `mode` is the access the
 * resolve() it descends from asked for. Size, entry count and modification
 * time are read from the disk each time.
 */
function fileObject(path: VirtualPath, mode: string, stat: Stat): any {
  var isDirectory = stat.kind === "directory";
  var parent = parentPath(path);
  var current = function () {
    return sandboxCall("stat", { path: path.full });
  };
  var file: any = {
    get parent() {
      if (!parent) {
        return null;
      }
      return fileObject(parent, mode, sandboxCall("stat", { path: parent.full }));
    },
    readOnly: stat.readOnly,
    isFile: !isDirectory,
    isDirectory: isDirectory,
    created: stat.created === null ? null : new Date(stat.created),
    get modified() {
      var modified = current().modified;
      return modified === null ? null : new Date(modified);
    },
    // music/ramones/ for music/ramones/volume1; a root's path is its name.
    path: parent ? parent.full + "/" : path.root,
    name: path.parts.length ? path.parts[path.parts.length - 1] : "",
    fullPath: path.full,
    // Undefined for a directory (#File::fileSize).
    get fileSize() {
      return isDirectory ? undefined : current().size;
    },
    // Undefined for a file (#File::length).
    get length() {
      return isDirectory ? current().entries : undefined;
    },

    toURI: function () {
      requirePrivilege(FS_READ_PRIVILEGE, "tizen.filesystem File.toURI");
      return fileUrl(path);
    },

    listFiles: function (onsuccess: (files: any[]) => void, onerror?: any, filter?: any) {
      requirePrivilege(FS_READ_PRIVILEGE, "tizen.filesystem File.listFiles");
      requireFunction(onsuccess, "onsuccess");
      requireOptionalFunction(onerror, "onerror");
      runLater(
        function () {
          requireDirectory();
          var entries = listDirectoryEntries(path, filter);
          var files: any[] = [];
          for (var i = 0; i < entries.length; i++) {
            files.push(fileObject(childPath(path, entries[i].name || "", "IOError"), mode, entries[i]));
          }
          return files;
        },
        onsuccess,
        onerror,
      );
    },

    openStream: function (streamMode: any, onsuccess: (stream: any) => void, onerror?: any, encoding?: any) {
      requirePrivilege(FS_READ_PRIVILEGE, "tizen.filesystem File.openStream");
      requireFileMode(streamMode);
      requireFunction(onsuccess, "onsuccess");
      requireOptionalFunction(onerror, "onerror");
      runLater(
        function () {
          var name = encodingName(encoding);
          if (!name) {
            throw new WebAPIException("InvalidValuesError", "Unsupported encoding: " + encoding);
          }
          var now = statOrNull(path);
          if (!now || now.kind !== "file") {
            throw new WebAPIException("IOError", path.full + " is not an existing file");
          }
          // "w" is for "[over]writing"; that it empties the file first, as
          // the 5.0 FileHandle's "w" does, is unverified.
          if (streamMode === "w") {
            sandboxCall("createFile", { path: path.full, ifExists: "truncate" });
          }
          return fileStream(path, streamMode, name);
        },
        onsuccess,
        onerror,
      );
    },

    readAsText: function (onsuccess: (text: string) => void, onerror?: any, encoding?: any) {
      requirePrivilege(FS_READ_PRIVILEGE, "tizen.filesystem File.readAsText");
      requireFunction(onsuccess, "onsuccess");
      requireOptionalFunction(onerror, "onerror");
      runLater(
        function () {
          var name = encodingName(encoding);
          if (!name) {
            throw new WebAPIException("InvalidValuesError", "Unsupported encoding: " + encoding);
          }
          if (isDirectory) {
            throw new WebAPIException("IOError", path.full + " is a directory");
          }
          return decodeText(readBytes(path, 0, null), name, null).text;
        },
        onsuccess,
        onerror,
      );
    },

    copyTo: function (origin: string, destination: string, overwrite: boolean, onsuccess?: any, onerror?: any) {
      transferFrom("copy", "File.copyTo", origin, destination, overwrite, onsuccess, onerror);
    },

    moveTo: function (origin: string, destination: string, overwrite: boolean, onsuccess?: any, onerror?: any) {
      transferFrom("move", "File.moveTo", origin, destination, overwrite, onsuccess, onerror);
    },

    createDirectory: function (dirPath: string) {
      requirePrivilege(FS_WRITE_PRIVILEGE, "tizen.filesystem File.createDirectory");
      requireDirectory();
      var child = childPath(path, dirPath, "InvalidValuesError");
      sandboxCall("createDirectory", { path: child.full, makeParents: true });
      return fileObject(child, mode, sandboxCall("stat", { path: child.full }));
    },

    createFile: function (relativeFilePath: string) {
      requirePrivilege(FS_WRITE_PRIVILEGE, "tizen.filesystem File.createFile");
      requireDirectory();
      var child = childPath(path, relativeFilePath, "InvalidValuesError");
      try {
        sandboxCall("createFile", { path: child.full });
      } catch (e) {
        // The reference lists no NotFoundError for createFile.
        if (e instanceof WebAPIException && e.name === "NotFoundError") {
          throw new WebAPIException("IOError", e.message);
        }
        throw e;
      }
      return fileObject(child, mode, sandboxCall("stat", { path: child.full }));
    },

    resolve: function (filePath: string) {
      requirePrivilege(FS_READ_PRIVILEGE, "tizen.filesystem File.resolve");
      requireDirectory();
      var child = childPath(path, filePath, "InvalidValuesError");
      return fileObject(child, mode, sandboxCall("stat", { path: child.full }));
    },

    deleteDirectory: function (directoryPath: string, recursive: boolean, onsuccess?: any, onerror?: any) {
      deleteBelow("directory", "File.deleteDirectory", directoryPath, !!recursive, onsuccess, onerror);
    },

    deleteFile: function (filePath: string, onsuccess?: any, onerror?: any) {
      deleteBelow("file", "File.deleteFile", filePath, false, onsuccess, onerror);
    },
  };

  function requireDirectory() {
    if (!isDirectory) {
      throw new WebAPIException("IOError", path.full + " is not a directory");
    }
  }

  /** Throws unless `target`, a full virtual path, is below this directory. */
  function requireBelow(target: any, errorName: string): VirtualPath {
    var below = requireVirtualPath(target, "tizen.filesystem File", errorName);
    if (below.full.indexOf(path.full + "/") !== 0) {
      throw new WebAPIException("InvalidValuesError", below.full + " is not under " + path.full);
    }
    return below;
  }

  function transferFrom(
    call: string,
    api: string,
    origin: any,
    destination: any,
    overwrite: boolean,
    onsuccess: any,
    onerror: any,
  ) {
    requirePrivilege(FS_WRITE_PRIVILEGE, "tizen.filesystem " + api);
    requireOptionalFunction(onsuccess, "onsuccess");
    requireOptionalFunction(onerror, "onerror");
    runLater(
      function () {
        requireDirectory();
        var from = requireBelow(origin, "InvalidValuesError");
        var to = requireVirtualPath(destination, "tizen.filesystem " + api, "InvalidValuesError");
        requireStorageAccess(to, "tizen.filesystem " + api);
        sandboxCall(call, { from: from.full, to: to.full, overwrite: !!overwrite });
      },
      function () {
        callIfFunction(onsuccess, []);
      },
      onerror,
    );
  }

  function deleteBelow(kind: string, api: string, target: any, recursive: boolean, onsuccess: any, onerror: any) {
    requirePrivilege(FS_WRITE_PRIVILEGE, "tizen.filesystem " + api);
    requireOptionalFunction(onsuccess, "onsuccess");
    requireOptionalFunction(onerror, "onerror");
    runLater(
      function () {
        requireDirectory();
        var below = requireBelow(target, "InvalidValuesError");
        sandboxCall("delete", { path: below.full, kind: kind, recursive: recursive });
      },
      function () {
        callIfFunction(onsuccess, []);
      },
      onerror,
    );
  }

  return file;
}

/**
 * A FileStream: reads and writes at `position`. Reading a stream opened
 * only for writing, or the reverse, throws IOError; the reference doesn't
 * say what happens (unverified). Once closed, any use throws a plain Error
 * (#FileStream).
 */
function fileStream(path: VirtualPath, mode: string, encoding: string): any {
  var size = sandboxCall("stat", { path: path.full }).size;
  var position = mode === "a" ? size : 0;
  var closed = false;

  function use(write: boolean, api: string) {
    if (closed) {
      throw new Error("The FileStream is closed");
    }
    requirePrivilege(write ? FS_WRITE_PRIVILEGE : FS_READ_PRIVILEGE, "tizen.filesystem FileStream." + api);
    if (write ? mode === "r" : mode === "w" || mode === "a") {
      throw new WebAPIException("IOError", "The FileStream was not opened for " + (write ? "writing" : "reading"));
    }
  }

  function requireCount(count: any): number {
    var n = Number(count);
    if (isNaN(n)) {
      throw new WebAPIException("TypeMismatchError", "The count must be a number");
    }
    if (n < 0) {
      throw new WebAPIException("InvalidValuesError", "The count must not be negative");
    }
    return Math.floor(n);
  }

  function write(bytes: ArrayLike<number>) {
    size = writeBytes(path, position, bytes);
    position += bytes.length;
  }

  return {
    get eof() {
      return position >= size;
    },
    get position() {
      return position;
    },
    set position(value: number) {
      position = Math.max(0, Math.floor(Number(value)) || 0);
    },
    get bytesAvailable() {
      return position >= size ? -1 : size - position;
    },

    close: function () {
      closed = true;
    },

    read: function (charCount: any) {
      use(false, "read");
      var count = requireCount(charCount);
      // A character takes at most 4 bytes in UTF-8.
      var bytes = readBytes(path, position, encoding === "UTF-8" ? count * 4 : count);
      var decoded = decodeText(bytes, encoding, count);
      position += decoded.bytes;
      return decoded.text;
    },

    readBytes: function (byteCount: any) {
      use(false, "readBytes");
      var bytes = readBytes(path, position, requireCount(byteCount));
      position += bytes.length;
      var octets: number[] = [];
      for (var i = 0; i < bytes.length; i++) {
        octets.push(bytes[i]);
      }
      return octets;
    },

    readBase64: function (byteCount: any) {
      use(false, "readBase64");
      var bytes = readBytes(path, position, requireCount(byteCount));
      position += bytes.length;
      return bytesToBase64(bytes);
    },

    write: function (stringData: any) {
      use(true, "write");
      write(encodeText(String(stringData), encoding));
    },

    writeBytes: function (byteData: any) {
      use(true, "writeBytes");
      if (!(byteData instanceof Array)) {
        throw new WebAPIException("TypeMismatchError", "byteData must be an array of octets");
      }
      write(byteData);
    },

    writeBase64: function (base64Data: any) {
      use(true, "writeBase64");
      var bytes: Uint8Array;
      try {
        bytes = base64ToBytes(String(base64Data));
      } catch (e) {
        throw new WebAPIException("InvalidValuesError", "base64Data is not valid Base64");
      }
      write(bytes);
    },
  };
}

var fileApiMethods = {
  resolve: function (location: any, onsuccess: (file: any) => void, onerror?: any, mode?: any) {
    requirePrivilege(FS_READ_PRIVILEGE, "tizen.filesystem.resolve");
    requireFunction(onsuccess, "onsuccess");
    requireOptionalFunction(onerror, "onerror");
    // rw when absent or null (#FileSystemManager::resolve).
    var access = mode === undefined || mode === null ? "rw" : requireFileMode(mode);
    var path = parseVirtualPath(location);
    if (path) {
      requireStorageAccess(path, "tizen.filesystem.resolve");
    }
    runLater(
      function () {
        var resolved = path || requireVirtualPath(location, "tizen.filesystem.resolve", "NotFoundError");
        // The read-only roots can only be resolved for reading.
        if (resolved.root === PACKAGE_ROOT && access !== "r") {
          throw new WebAPIException("InvalidValuesError", PACKAGE_ROOT + " is read-only; resolve it with mode r");
        }
        return fileObject(resolved, access, sandboxCall("stat", { path: resolved.full }));
      },
      onsuccess,
      onerror,
    );
  },
};
