//! Watches a Project Folder or a Package for hot reload and groups bursts of
//! changes (an editor's save-all, an atomic save's temporary file) into batches.

use notify::{EventKind, RecursiveMode, Watcher as _};
use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::Instant;

/// A batch is sent once the folder has been quiet this long...
const QUIET: Duration = Duration::from_millis(150);
/// ...or this long after its first change, whichever comes first.
const MAX_WAIT: Duration = Duration::from_secs(1);

/// The App files that changed together, relative to the watched folder and
/// `/`-separated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Batch {
    pub paths: Vec<String>,
}

/// What a batch of changes calls for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Only stylesheets changed: swap them in place.
    SwapStyles,
    /// Another asset changed: reload the App.
    Reload,
    /// `config.xml` changed: re-read the App and restart it.
    Restart,
}

impl Batch {
    pub fn action(&self) -> Action {
        if self.paths.iter().any(|p| p == "config.xml") {
            Action::Restart
        } else if self.paths.iter().all(|p| has_extension(p, "css")) {
            Action::SwapStyles
        } else {
            Action::Reload
        }
    }
}

/// A running watch; dropping it stops watching.
pub struct SourceWatcher {
    _watcher: notify::RecommendedWatcher,
    batches: mpsc::UnboundedReceiver<Batch>,
}

impl SourceWatcher {
    pub fn start(root: &Path) -> notify::Result<SourceWatcher> {
        Self::watch(root, RecursiveMode::Recursive, None)
    }

    /// Watches a single file, such as a Package; batches name just that file.
    /// Its folder is watched rather than the file itself, so a build that
    /// deletes the file and writes a new one is still followed.
    pub fn start_file(file: &Path) -> notify::Result<SourceWatcher> {
        // Absolute first: a bare `app.wgt` has no parent folder to watch.
        let file = canonical_without_verbatim_prefix(&std::path::absolute(file)?);
        let (Some(folder), Some(name)) = (file.parent(), file.file_name().and_then(|n| n.to_str()))
        else {
            return Err(notify::Error::path_not_found().add_path(file));
        };
        Self::watch(folder, RecursiveMode::NonRecursive, Some(name.to_string()))
    }

    fn watch(
        root: &Path,
        mode: RecursiveMode,
        only_file_name: Option<String>,
    ) -> notify::Result<SourceWatcher> {
        // Events carry absolute paths, which on Windows may be in a different
        // form than `root`; canonical forms on both sides make them comparable.
        let root = canonical_without_verbatim_prefix(root);
        let (changes_tx, changes) = mpsc::unbounded_channel::<PathBuf>();
        let mut watcher =
            notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
                let Ok(event) = event else { return };
                if matches!(event.kind, EventKind::Access(_)) {
                    return;
                }
                for path in event.paths {
                    let _ = changes_tx.send(path);
                }
            })?;
        watcher.watch(&root, mode)?;
        let (batches_tx, batches) = mpsc::unbounded_channel();
        tokio::spawn(debounce(root, only_file_name, changes, batches_tx));
        Ok(SourceWatcher {
            _watcher: watcher,
            batches,
        })
    }

    /// The next batch of changes, or `None` once watching has stopped.
    pub async fn next(&mut self) -> Option<Batch> {
        self.batches.recv().await
    }
}

/// Groups changes into batches. With `only_file_name`, changes to anything but
/// that file in `root` are ignored.
async fn debounce(
    root: PathBuf,
    only_file_name: Option<String>,
    mut changes: mpsc::UnboundedReceiver<PathBuf>,
    batches: mpsc::UnboundedSender<Batch>,
) {
    let relevant = |path: &Path| {
        app_path(&root, path).filter(|p| only_file_name.as_ref().is_none_or(|name| p == name))
    };
    while let Some(first) = changes.recv().await {
        let deadline = Instant::now() + MAX_WAIT;
        let mut paths = BTreeSet::new();
        paths.extend(relevant(&first));
        loop {
            let wait = QUIET.min(deadline.saturating_duration_since(Instant::now()));
            match tokio::time::timeout(wait, changes.recv()).await {
                Ok(Some(path)) => paths.extend(relevant(&path)),
                Ok(None) | Err(_) => break,
            }
        }
        if !paths.is_empty()
            && batches
                .send(Batch {
                    paths: paths.into_iter().collect(),
                })
                .is_err()
        {
            return;
        }
    }
}

/// The path of a changed file relative to the Project Folder, or `None` for
/// changes that aren't App files: the root itself, hidden files and folders
/// (`.git`, editor swap files) and editor backups ending in `~`.
fn app_path(root: &Path, path: &Path) -> Option<String> {
    let relative = path
        .strip_prefix(root)
        .ok()
        .map(Path::to_path_buf)
        .or_else(|| {
            canonical_without_verbatim_prefix(path)
                .strip_prefix(root)
                .ok()
                .map(Path::to_path_buf)
        })?;
    let mut parts = Vec::new();
    for component in relative.components() {
        let Component::Normal(part) = component else {
            return None;
        };
        let part = part.to_str()?;
        if part.starts_with('.') || part.ends_with('~') {
            return None;
        }
        parts.push(part);
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

fn has_extension(path: &str, extension: &str) -> bool {
    Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case(extension))
}

/// The canonical form of a path without Windows' `\\?\` prefix, which file
/// change events don't use. Falls back to the path itself (e.g. once deleted).
fn canonical_without_verbatim_prefix(path: &Path) -> PathBuf {
    let Ok(canonical) = path.canonicalize() else {
        return path.to_path_buf();
    };
    match canonical.to_str().and_then(|s| s.strip_prefix(r"\\?\")) {
        Some(stripped) if !stripped.starts_with("UNC\\") => PathBuf::from(stripped),
        _ => canonical,
    }
}
