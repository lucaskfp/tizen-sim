//! An App's Sandbox: the directory on the developer's disk that stands in for
//! the TV's storage of the App's files, under Tizen's virtual roots
//! (filesystem.html). The `tizen.filesystem` Shims reach it over the Bridge.
//!
//! Paths are virtual: `<root>/<path>`, e.g. `wgt-private/notes.txt`. Nothing
//! outside the Sandbox, and the App's own files, can be reached.

use crate::home::Home;
use base64::Engine as _;
use serde_json::{Value, json};
use std::fs::OpenOptions;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

/// The virtual roots a TV has (filesystem.html; `ringtones` is not
/// supported on TV, TD guides/data/file-system.md).
pub const ROOTS: &[&str] = &[
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

/// The read-only root holding the App's own files.
const PACKAGE_ROOT: &str = "wgt-package";

#[derive(Debug, thiserror::Error)]
pub enum ResetError {
    #[error("{0} is running; stop its Session first")]
    Running(String),
    #[error("could not clear {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
}

/// Clears what an App has stored, so its next launch is a first launch: its
/// Sandbox, and its Engine data (web storage, IndexedDB, cookies) on every
/// Version Profile. Returns whether there was anything to clear. Refused
/// while the App runs, since its Engine holds that data open.
pub fn reset(app_id: &str, home: &Home) -> Result<bool, ResetError> {
    let running = crate::registry::SessionRegistry::new(home).list();
    if running.iter().any(|entry| entry.app_id == app_id) {
        return Err(ResetError::Running(app_id.to_string()));
    }
    let mut cleared = false;
    for dir in [home.sandbox_dir(app_id), home.engine_data_dir(app_id)] {
        match std::fs::remove_dir_all(&dir) {
            Ok(()) => cleared = true,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(source) => return Err(ResetError::Io { path: dir, source }),
        }
    }
    Ok(cleared)
}

/// A failed Sandbox call, named as the WebAPI error the Shim raises. The
/// names follow filesystem.html's error lists: NotFoundError for what is
/// missing, IOError for other I/O failures (an existing target, a read-only
/// location), InvalidValuesError for an invalid path. Where a method's list
/// doesn't fit a case, the Shim renames the error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxError {
    pub name: &'static str,
    pub message: String,
}

fn error(name: &'static str, message: impl Into<String>) -> SandboxError {
    SandboxError {
        name,
        message: message.into(),
    }
}

fn io_error(path: &str, e: std::io::Error) -> SandboxError {
    match e.kind() {
        std::io::ErrorKind::NotFound => error("NotFoundError", format!("{path} does not exist")),
        _ => error("IOError", format!("{path}: {e}")),
    }
}

pub struct Sandbox {
    dir: PathBuf,
    package: Option<PathBuf>,
}

/// A virtual path resolved to the disk.
struct Resolved {
    virtual_path: String,
    disk: PathBuf,
    read_only: bool,
}

impl Sandbox {
    /// A Sandbox kept in `dir`, whose read-only `wgt-package` root is the
    /// App's own files in `package` (none for a Dev Server URL App).
    pub fn new(dir: PathBuf, package: Option<PathBuf>) -> Sandbox {
        Sandbox { dir, package }
    }

    /// Readies the Sandbox for a new Session: `wgt-private-tmp` holds data
    /// only "during a widget execution cycle" (filesystem.html, resolve), so
    /// it starts empty.
    pub fn start_session(&self) -> std::io::Result<()> {
        match std::fs::remove_dir_all(self.dir.join("wgt-private-tmp")) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        }
    }

    /// Runs one Bridge call (`filesystem.<method>`) on the Sandbox.
    pub fn call(&self, method: &str, params: &Value) -> Result<Value, SandboxError> {
        let path = || self.resolve(params["path"].as_str().unwrap_or_default());
        match method {
            "createFile" => self.create_file(&path()?, params),
            "write" => write(&path()?, params),
            "read" => read(&path()?, params),
            "stat" => stat(&path()?),
            "list" => list(&path()?),
            "createDirectory" => create_directory(&path()?, params),
            "delete" => delete(&path()?, params),
            "copy" | "move" => {
                let from = self.resolve(params["from"].as_str().unwrap_or_default())?;
                let to = self.resolve(params["to"].as_str().unwrap_or_default())?;
                transfer(&from, &to, params, method == "move")
            }
            _ => Err(error("NotSupportedError", format!("unknown call {method}"))),
        }
    }

    /// Maps a virtual path to the disk, refusing anything outside the roots.
    fn resolve(&self, virtual_path: &str) -> Result<Resolved, SandboxError> {
        let mut parts = virtual_path.split('/').filter(|part| !part.is_empty());
        let root = parts.next().unwrap_or_default();
        if !ROOTS.contains(&root) {
            return Err(error(
                "InvalidValuesError",
                format!("{virtual_path} is not under a virtual root"),
            ));
        }
        let rest: Vec<&str> = parts.collect();
        if rest
            .iter()
            .any(|part| *part == "." || *part == ".." || part.contains('\\'))
        {
            return Err(error(
                "InvalidValuesError",
                format!("{virtual_path} is not a valid path"),
            ));
        }
        let base = if root == PACKAGE_ROOT {
            self.package.clone().ok_or_else(|| {
                error(
                    "IOError",
                    "wgt-package is not available for a Dev Server URL App: its files are the \
                     bundler's",
                )
            })?
        } else {
            // The reference lists the roots a TV "must" support, so each
            // exists from the start (filesystem.html).
            let base = self.dir.join(root);
            std::fs::create_dir_all(&base).map_err(|e| io_error(root, e))?;
            base
        };
        let mut disk = base;
        disk.extend(&rest);
        let mut normalized = vec![root];
        normalized.extend(&rest);
        Ok(Resolved {
            virtual_path: normalized.join("/"),
            disk,
            read_only: root == PACKAGE_ROOT,
        })
    }

    fn create_file(&self, at: &Resolved, params: &Value) -> Result<Value, SandboxError> {
        writable(at)?;
        prepare_parent(at, params["makeParents"].as_bool().unwrap_or(false))?;
        let mut options = OpenOptions::new();
        options.write(true);
        match params["ifExists"].as_str().unwrap_or("fail") {
            "truncate" => options.create(true).truncate(true),
            "keep" => options.create(true),
            _ => options.create_new(true),
        };
        options.open(&at.disk).map_err(|e| match e.kind() {
            std::io::ErrorKind::AlreadyExists => {
                error("IOError", format!("{} already exists", at.virtual_path))
            }
            _ => io_error(&at.virtual_path, e),
        })?;
        Ok(json!(at.virtual_path))
    }
}

fn writable(at: &Resolved) -> Result<(), SandboxError> {
    if at.read_only {
        return Err(error(
            "IOError",
            format!("{} is in a read-only location", at.virtual_path),
        ));
    }
    Ok(())
}

/// Creates the parent directories of `at` if allowed, or checks they exist.
fn prepare_parent(at: &Resolved, make_parents: bool) -> Result<(), SandboxError> {
    let Some(parent) = at.disk.parent() else {
        return Ok(());
    };
    if make_parents || directly_in_root(at) {
        std::fs::create_dir_all(parent).map_err(|e| io_error(&at.virtual_path, e))
    } else if parent.is_dir() {
        Ok(())
    } else {
        Err(error(
            "NotFoundError",
            format!("the directory of {} does not exist", at.virtual_path),
        ))
    }
}

/// Whether `at` sits directly in a virtual root, whose directory the
/// Sandbox creates on first use.
fn directly_in_root(at: &Resolved) -> bool {
    at.virtual_path.matches('/').count() == 1
}

fn decode(data: &Value) -> Result<Vec<u8>, SandboxError> {
    base64::engine::general_purpose::STANDARD
        .decode(data.as_str().unwrap_or_default())
        .map_err(|e| error("InvalidValuesError", format!("bad data: {e}")))
}

fn write(at: &Resolved, params: &Value) -> Result<Value, SandboxError> {
    writable(at)?;
    let data = decode(&params["data"])?;
    let mut file = OpenOptions::new()
        .write(true)
        .open(&at.disk)
        .map_err(|e| io_error(&at.virtual_path, e))?;
    let offset = params["offset"].as_u64().unwrap_or(0);
    file.seek(SeekFrom::Start(offset))
        .and_then(|_| file.write_all(&data))
        .map_err(|e| io_error(&at.virtual_path, e))?;
    let size = file
        .metadata()
        .map_err(|e| io_error(&at.virtual_path, e))?
        .len();
    Ok(json!({ "size": size }))
}

fn read(at: &Resolved, params: &Value) -> Result<Value, SandboxError> {
    let mut file = std::fs::File::open(&at.disk).map_err(|e| io_error(&at.virtual_path, e))?;
    if file.metadata().is_ok_and(|m| m.is_dir()) {
        return Err(error(
            "IOError",
            format!("{} is a directory", at.virtual_path),
        ));
    }
    let offset = params["offset"].as_u64().unwrap_or(0);
    let mut data = vec![];
    file.seek(SeekFrom::Start(offset))
        .and_then(|_| match params["length"].as_u64() {
            Some(length) => file.take(length).read_to_end(&mut data),
            None => file.read_to_end(&mut data),
        })
        .map_err(|e| io_error(&at.virtual_path, e))?;
    Ok(json!({ "data": base64::engine::general_purpose::STANDARD.encode(data) }))
}

fn create_directory(at: &Resolved, params: &Value) -> Result<Value, SandboxError> {
    writable(at)?;
    prepare_parent(at, params["makeParents"].as_bool().unwrap_or(false))?;
    if at.disk.is_dir() {
        if params["ifExists"] == "keep" {
            return Ok(json!(at.virtual_path));
        }
        return Err(error(
            "IOError",
            format!("{} already exists", at.virtual_path),
        ));
    }
    std::fs::create_dir(&at.disk).map_err(|e| match e.kind() {
        std::io::ErrorKind::AlreadyExists => {
            error("IOError", format!("{} already exists", at.virtual_path))
        }
        _ => io_error(&at.virtual_path, e),
    })?;
    Ok(json!(at.virtual_path))
}

/// A directory's entries, each described as by `stat` plus its `name`.
fn list(at: &Resolved) -> Result<Value, SandboxError> {
    if at.disk.is_file() {
        return Err(error(
            "IOError",
            format!("{} is not a directory", at.virtual_path),
        ));
    }
    let entries = std::fs::read_dir(&at.disk).map_err(|e| io_error(&at.virtual_path, e))?;
    let mut listing = vec![];
    for entry in entries.flatten() {
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        let mut described = describe(&metadata, at.read_only, Value::Null);
        described["name"] = json!(entry.file_name().to_string_lossy());
        listing.push(described);
    }
    Ok(Value::Array(listing))
}

/// Deletes the file or directory (`kind`) at the path; a non-empty directory
/// only when `recursive` (#FileSystemManager::deleteDirectory). That a
/// virtual root itself can't be deleted is unverified.
fn delete(at: &Resolved, params: &Value) -> Result<Value, SandboxError> {
    writable(at)?;
    let directory = params["kind"] == "directory";
    if is_root(at) {
        return Err(error(
            "IOError",
            format!("{} is a virtual root", at.virtual_path),
        ));
    }
    let found = if directory {
        at.disk.is_dir()
    } else {
        at.disk.is_file()
    };
    if !found {
        let what = if directory { "directory" } else { "file" };
        return Err(error(
            "NotFoundError",
            format!("{} is not an existing {what}", at.virtual_path),
        ));
    }
    let result = match (directory, params["recursive"].as_bool().unwrap_or(false)) {
        (false, _) => std::fs::remove_file(&at.disk),
        (true, true) => std::fs::remove_dir_all(&at.disk),
        (true, false) => {
            let empty = std::fs::read_dir(&at.disk).is_ok_and(|mut e| e.next().is_none());
            if !empty {
                return Err(error(
                    "IOError",
                    format!("{} is not empty", at.virtual_path),
                ));
            }
            std::fs::remove_dir(&at.disk)
        }
    };
    result.map_err(|e| io_error(&at.virtual_path, e))?;
    Ok(json!(at.virtual_path))
}

/// Copies (or moves) the file or directory at `from` to exactly `to`. A
/// directory merges into one already at `to`, and what is in the way is
/// replaced only when `overwrite` is set (#FileSystemManager::copyDirectory,
/// moveDirectory; copyFile and File.copyTo for files). Without it the copy
/// stops at the first conflict, keeping what it copied so far; a missing
/// destination directory is IOError for a copy (both measured on a Tizen 5.0
/// TV) and NotFoundError for a move (#FileSystemManager::moveFile). That a
/// virtual root can't be copied or moved is unverified. `kind`, if given, is
/// what `from` must be.
fn transfer(
    from: &Resolved,
    to: &Resolved,
    params: &Value,
    remove_source: bool,
) -> Result<Value, SandboxError> {
    writable(to)?;
    if remove_source {
        writable(from)?;
    }
    let found = match params["kind"].as_str() {
        Some("file") => from.disk.is_file(),
        Some("directory") => from.disk.is_dir(),
        _ => from.disk.exists(),
    };
    if !found {
        return Err(error(
            "NotFoundError",
            format!("{} does not exist", from.virtual_path),
        ));
    }
    if is_root(from) || is_root(to) {
        return Err(error("IOError", "a virtual root can't be copied or moved"));
    }
    if to.disk.starts_with(&from.disk) {
        return Err(error(
            "IOError",
            format!("{} can't go inside itself", from.virtual_path),
        ));
    }
    if !to.disk.parent().is_some_and(Path::is_dir) {
        return Err(error(
            if remove_source {
                "NotFoundError"
            } else {
                "IOError"
            },
            format!("the directory of {} does not exist", to.virtual_path),
        ));
    }
    let overwrite = params["overwrite"].as_bool().unwrap_or(false);
    copy_over(&from.disk, &to.disk, overwrite).map_err(|e| match e.kind() {
        std::io::ErrorKind::AlreadyExists => error(
            "IOError",
            format!("{} would overwrite what is already there", to.virtual_path),
        ),
        _ => io_error(&from.virtual_path, e),
    })?;
    if remove_source {
        let removed = if from.disk.is_dir() {
            std::fs::remove_dir_all(&from.disk)
        } else {
            std::fs::remove_file(&from.disk)
        };
        removed.map_err(|e| io_error(&from.virtual_path, e))?;
    }
    Ok(json!(to.virtual_path))
}

/// Copies `from` onto `to`, merging directories, in name order. What is in
/// the way is replaced when `overwrite` is set; otherwise the copy stops
/// there with `AlreadyExists`.
fn copy_over(from: &Path, to: &Path, overwrite: bool) -> std::io::Result<()> {
    let in_the_way = || {
        if overwrite {
            Ok(())
        } else {
            Err(std::io::Error::from(std::io::ErrorKind::AlreadyExists))
        }
    };
    if from.is_dir() {
        if to.is_file() {
            in_the_way()?;
            std::fs::remove_file(to)?;
        }
        std::fs::create_dir_all(to)?;
        let mut entries: Vec<_> = std::fs::read_dir(from)?.collect::<Result<_, _>>()?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            copy_over(&entry.path(), &to.join(entry.file_name()), overwrite)?;
        }
        Ok(())
    } else {
        if to.exists() {
            in_the_way()?;
            if to.is_dir() {
                std::fs::remove_dir_all(to)?;
            }
        }
        std::fs::copy(from, to).map(|_| ())
    }
}

fn is_root(at: &Resolved) -> bool {
    !at.virtual_path.contains('/')
}

/// What a file or directory is: `kind` (`file` or `directory`), `size` in
/// bytes, `entries` (a directory's), `created` and `modified` in
/// milliseconds since the epoch (`null` where the disk doesn't record
/// them), and `readOnly`.
fn stat(at: &Resolved) -> Result<Value, SandboxError> {
    let metadata = std::fs::metadata(&at.disk).map_err(|e| io_error(&at.virtual_path, e))?;
    let entries = if metadata.is_dir() {
        let entries = std::fs::read_dir(&at.disk).map_err(|e| io_error(&at.virtual_path, e))?;
        json!(entries.count())
    } else {
        Value::Null
    };
    Ok(describe(&metadata, at.read_only, entries))
}

fn describe(metadata: &std::fs::Metadata, read_only: bool, entries: Value) -> Value {
    let millis = |time: std::io::Result<std::time::SystemTime>| {
        time.ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as u64)
    };
    json!({
        "kind": if metadata.is_dir() { "directory" } else { "file" },
        "size": metadata.len(),
        "entries": entries,
        "created": millis(metadata.created()),
        "modified": millis(metadata.modified()),
        "readOnly": read_only,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sandbox() -> (tempfile::TempDir, Sandbox) {
        let dir = tempfile::tempdir().unwrap();
        let sandbox = Sandbox::new(dir.path().join("sandbox"), None);
        (dir, sandbox)
    }

    fn b64(bytes: &[u8]) -> String {
        base64::engine::general_purpose::STANDARD.encode(bytes)
    }

    #[test]
    fn a_written_file_reads_back_and_lives_under_its_virtual_root() {
        let (dir, sandbox) = sandbox();
        let path = json!("wgt-private/notes/today.txt");
        sandbox
            .call(
                "createFile",
                &json!({ "path": path, "makeParents": true, "ifExists": "fail" }),
            )
            .unwrap();
        sandbox
            .call(
                "write",
                &json!({ "path": path, "offset": 0, "data": b64(b"hello world") }),
            )
            .unwrap();
        sandbox
            .call(
                "write",
                &json!({ "path": path, "offset": 6, "data": b64(b"tizen") }),
            )
            .unwrap();

        let read = sandbox
            .call("read", &json!({ "path": path, "offset": 0 }))
            .unwrap();
        assert_eq!(read["data"], b64(b"hello tizen"));
        let tail = sandbox
            .call("read", &json!({ "path": path, "offset": 6, "length": 3 }))
            .unwrap();
        assert_eq!(tail["data"], b64(b"tiz"));
        assert_eq!(
            std::fs::read(dir.path().join("sandbox/wgt-private/notes/today.txt")).unwrap(),
            b"hello tizen"
        );
    }

    #[test]
    fn wgt_package_is_the_apps_own_files_and_read_only() {
        let dir = tempfile::tempdir().unwrap();
        let package = dir.path().join("app");
        std::fs::create_dir_all(&package).unwrap();
        std::fs::write(package.join("index.html"), "<html>").unwrap();
        let sandbox = Sandbox::new(dir.path().join("sandbox"), Some(package.clone()));

        let read = sandbox
            .call("read", &json!({ "path": "wgt-package/index.html" }))
            .unwrap();
        assert_eq!(read["data"], b64(b"<html>"));
        let stat = sandbox
            .call("stat", &json!({ "path": "wgt-package/index.html" }))
            .unwrap();
        assert_eq!(stat["readOnly"], true);

        let refused = sandbox
            .call(
                "write",
                &json!({ "path": "wgt-package/index.html", "offset": 0, "data": b64(b"x") }),
            )
            .unwrap_err();
        assert_eq!(refused.name, "IOError");
        assert_eq!(
            std::fs::read(package.join("index.html")).unwrap(),
            b"<html>"
        );
    }

    #[test]
    fn nothing_outside_the_virtual_roots_can_be_reached() {
        let (dir, sandbox) = sandbox();
        std::fs::write(dir.path().join("secret"), "x").unwrap();
        for path in [
            "wgt-private/../../secret",
            "wgt-private/..\\..\\secret",
            "/etc/passwd",
            "file:///tmp/x",
            "ringtones/a.mp3",
        ] {
            let refused = sandbox.call("read", &json!({ "path": path })).unwrap_err();
            assert_eq!(refused.name, "InvalidValuesError", "{path}");
        }
    }

    #[test]
    fn stat_describes_files_and_directories() {
        let (_dir, sandbox) = sandbox();
        sandbox
            .call(
                "createFile",
                &json!({ "path": "documents/a/b.txt", "makeParents": true }),
            )
            .unwrap();
        sandbox
            .call(
                "write",
                &json!({ "path": "documents/a/b.txt", "data": b64(b"12345") }),
            )
            .unwrap();

        let file = sandbox
            .call("stat", &json!({ "path": "documents/a/b.txt" }))
            .unwrap();
        assert_eq!(file["kind"], "file");
        assert_eq!(file["size"], 5);
        assert_eq!(file["readOnly"], false);
        assert!(file["modified"].as_f64().unwrap() > 0.0);
        let directory = sandbox
            .call("stat", &json!({ "path": "documents/a" }))
            .unwrap();
        assert_eq!(directory["kind"], "directory");
        assert_eq!(directory["entries"], 1);
        // A virtual root always exists, even before anything is written to it.
        let root = sandbox.call("stat", &json!({ "path": "music" })).unwrap();
        assert_eq!(root["kind"], "directory");
        assert_eq!(root["entries"], 0);

        let missing = sandbox
            .call("stat", &json!({ "path": "documents/nope" }))
            .unwrap_err();
        assert_eq!(missing.name, "NotFoundError");
    }

    fn names(listing: &Value) -> Vec<String> {
        let mut names: Vec<String> = listing
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| {
                format!(
                    "{} {}",
                    entry["kind"].as_str().unwrap(),
                    entry["name"].as_str().unwrap()
                )
            })
            .collect();
        names.sort();
        names
    }

    #[test]
    fn directories_are_created_listed_and_deleted() {
        let (_dir, sandbox) = sandbox();
        let created = sandbox
            .call(
                "createDirectory",
                &json!({ "path": "documents/a/b/", "makeParents": true }),
            )
            .unwrap();
        assert_eq!(created, "documents/a/b");
        sandbox
            .call("createFile", &json!({ "path": "documents/a/f.txt" }))
            .unwrap();
        // Without makeParents the parent must exist.
        let orphan = sandbox
            .call("createDirectory", &json!({ "path": "documents/x/y" }))
            .unwrap_err();
        assert_eq!(orphan.name, "NotFoundError");
        let again = sandbox
            .call("createDirectory", &json!({ "path": "documents/a" }))
            .unwrap_err();
        assert_eq!(again.name, "IOError");
        sandbox
            .call(
                "createDirectory",
                &json!({ "path": "documents/a", "ifExists": "keep" }),
            )
            .unwrap();

        let listing = sandbox
            .call("list", &json!({ "path": "documents/a" }))
            .unwrap();
        assert_eq!(names(&listing), ["directory b", "file f.txt"]);
        let of_a_file = sandbox
            .call("list", &json!({ "path": "documents/a/f.txt" }))
            .unwrap_err();
        assert_eq!(of_a_file.name, "IOError");

        // A non-empty directory goes only when the deletion is recursive.
        let not_empty = sandbox
            .call(
                "delete",
                &json!({ "path": "documents/a", "kind": "directory", "recursive": false }),
            )
            .unwrap_err();
        assert_eq!(not_empty.name, "IOError");
        let not_a_file = sandbox
            .call("delete", &json!({ "path": "documents/a", "kind": "file" }))
            .unwrap_err();
        assert_eq!(not_a_file.name, "NotFoundError");
        sandbox
            .call(
                "delete",
                &json!({ "path": "documents/a/f.txt", "kind": "file" }),
            )
            .unwrap();
        sandbox
            .call(
                "delete",
                &json!({ "path": "documents/a", "kind": "directory", "recursive": true }),
            )
            .unwrap();
        let listing = sandbox
            .call("list", &json!({ "path": "documents" }))
            .unwrap();
        assert!(names(&listing).is_empty());
        let root = sandbox
            .call(
                "delete",
                &json!({ "path": "documents", "kind": "directory", "recursive": true }),
            )
            .unwrap_err();
        assert_eq!(root.name, "IOError");
    }

    fn put(sandbox: &Sandbox, path: &str, text: &str) {
        sandbox
            .call(
                "createFile",
                &json!({ "path": path, "makeParents": true, "ifExists": "truncate" }),
            )
            .unwrap();
        sandbox
            .call(
                "write",
                &json!({ "path": path, "data": b64(text.as_bytes()) }),
            )
            .unwrap();
    }

    fn text(sandbox: &Sandbox, path: &str) -> String {
        let read = sandbox.call("read", &json!({ "path": path })).unwrap();
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(read["data"].as_str().unwrap())
            .unwrap();
        String::from_utf8(bytes).unwrap()
    }

    fn exists(sandbox: &Sandbox, path: &str) -> bool {
        sandbox.call("stat", &json!({ "path": path })).is_ok()
    }

    #[test]
    fn files_are_copied_and_moved_overwriting_only_when_asked() {
        let (_dir, sandbox) = sandbox();
        put(&sandbox, "documents/a.txt", "A");
        put(&sandbox, "documents/b.txt", "B");

        let refused = sandbox
            .call(
                "copy",
                &json!({ "from": "documents/a.txt", "to": "documents/b.txt" }),
            )
            .unwrap_err();
        assert_eq!(refused.name, "IOError");
        let copied = sandbox
            .call(
                "copy",
                &json!({ "from": "documents/a.txt", "to": "documents/b.txt", "overwrite": true }),
            )
            .unwrap();
        assert_eq!(copied, "documents/b.txt");
        assert_eq!(text(&sandbox, "documents/b.txt"), "A");

        sandbox
            .call(
                "move",
                &json!({ "from": "documents/a.txt", "to": "images/c.txt" }),
            )
            .unwrap();
        assert!(!exists(&sandbox, "documents/a.txt"));
        assert_eq!(text(&sandbox, "images/c.txt"), "A");

        let missing = sandbox
            .call(
                "copy",
                &json!({ "from": "documents/nope", "to": "documents/x" }),
            )
            .unwrap_err();
        assert_eq!(missing.name, "NotFoundError");
        let no_directory = sandbox
            .call(
                "copy",
                &json!({ "from": "documents/b.txt", "to": "documents/no/x" }),
            )
            .unwrap_err();
        // Measured on a Tizen 5.0 TV: copyFile gives IOError here.
        assert_eq!(no_directory.name, "IOError");
        let wrong_kind = sandbox
            .call(
                "copy",
                &json!({ "from": "documents", "to": "music/d", "kind": "file" }),
            )
            .unwrap_err();
        assert_eq!(wrong_kind.name, "NotFoundError");
    }

    #[test]
    fn directories_merge_into_an_existing_destination() {
        let (_dir, sandbox) = sandbox();
        put(&sandbox, "documents/src/a.txt", "a");
        put(&sandbox, "documents/src/one.txt", "new one");
        put(&sandbox, "documents/src/sub/two.txt", "two");
        put(&sandbox, "documents/dst/one.txt", "old one");
        put(&sandbox, "documents/dst/keep.txt", "keep");

        // Without overwrite the copy stops at the first conflict, keeping
        // what it copied before it, as a Tizen 5.0 TV does.
        let conflict = sandbox
            .call(
                "copy",
                &json!({ "from": "documents/src", "to": "documents/dst" }),
            )
            .unwrap_err();
        assert_eq!(conflict.name, "IOError");
        assert_eq!(text(&sandbox, "documents/dst/a.txt"), "a");
        assert_eq!(text(&sandbox, "documents/dst/one.txt"), "old one");
        sandbox
            .call(
                "move",
                &json!({ "from": "documents/src", "to": "documents/dst", "overwrite": true }),
            )
            .unwrap();
        assert!(!exists(&sandbox, "documents/src"));
        assert_eq!(text(&sandbox, "documents/dst/one.txt"), "new one");
        assert_eq!(text(&sandbox, "documents/dst/sub/two.txt"), "two");
        assert_eq!(text(&sandbox, "documents/dst/keep.txt"), "keep");

        let into_itself = sandbox
            .call(
                "copy",
                &json!({ "from": "documents/dst", "to": "documents/dst/sub/x" }),
            )
            .unwrap_err();
        assert_eq!(into_itself.name, "IOError");
    }

    #[test]
    fn a_new_session_starts_with_an_empty_temporary_root() {
        let (_dir, sandbox) = sandbox();
        put(&sandbox, "wgt-private-tmp/cache.json", "{}");
        put(&sandbox, "wgt-private/settings.json", "{}");

        sandbox.start_session().unwrap();

        assert!(!exists(&sandbox, "wgt-private-tmp/cache.json"));
        assert!(exists(&sandbox, "wgt-private/settings.json"));
    }

    #[test]
    fn reset_clears_the_apps_sandbox_and_engine_data_only() {
        let home_dir = tempfile::tempdir().unwrap();
        let home = Home::at(home_dir.path());
        for app in ["abcdefghij.App", "klmnopqrst.Other"] {
            std::fs::create_dir_all(home.sandbox_dir(app).join("wgt-private")).unwrap();
            std::fs::create_dir_all(home.engine_data_dir(app).join("10.0")).unwrap();
        }

        let cleared = reset("abcdefghij.App", &home).unwrap();

        assert!(cleared);
        assert!(!home.sandbox_dir("abcdefghij.App").exists());
        assert!(!home.engine_data_dir("abcdefghij.App").exists());
        assert!(home.sandbox_dir("klmnopqrst.Other").exists());
        assert!(home.engine_data_dir("klmnopqrst.Other").exists());
        assert!(
            !reset("abcdefghij.App", &home).unwrap(),
            "nothing left to clear"
        );
    }
}
