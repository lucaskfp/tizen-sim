//! Watches a Project Folder for hot reload and groups bursts of changes (an
//! editor's save-all, an atomic save's temporary file) into single batches.

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

/// The App files that changed together, relative to the Project Folder and
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
pub struct FolderWatcher {
    _watcher: notify::RecommendedWatcher,
    batches: mpsc::UnboundedReceiver<Batch>,
}

impl FolderWatcher {
    pub fn start(root: &Path) -> notify::Result<FolderWatcher> {
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
        watcher.watch(&root, RecursiveMode::Recursive)?;
        let (batches_tx, batches) = mpsc::unbounded_channel();
        tokio::spawn(debounce(root, changes, batches_tx));
        Ok(FolderWatcher {
            _watcher: watcher,
            batches,
        })
    }

    /// The next batch of changes, or `None` once watching has stopped.
    pub async fn next(&mut self) -> Option<Batch> {
        self.batches.recv().await
    }
}

async fn debounce(
    root: PathBuf,
    mut changes: mpsc::UnboundedReceiver<PathBuf>,
    batches: mpsc::UnboundedSender<Batch>,
) {
    while let Some(first) = changes.recv().await {
        let deadline = Instant::now() + MAX_WAIT;
        let mut paths = BTreeSet::new();
        paths.extend(app_path(&root, &first));
        loop {
            let wait = QUIET.min(deadline.saturating_duration_since(Instant::now()));
            match tokio::time::timeout(wait, changes.recv()).await {
                Ok(Some(path)) => paths.extend(app_path(&root, &path)),
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
