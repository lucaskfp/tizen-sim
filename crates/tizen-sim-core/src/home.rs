//! Where the simulator keeps its per-user files.

use crate::engine::EngineStore;
use std::path::{Path, PathBuf};

/// The per-user directories: the Engine cache and per-App state (Engine
/// data directories, remembered settings).
///
/// Defaults to `~/.tizen-sim`, overridable with `TIZEN_SIM_HOME`. The home
/// directory is used instead of `%LOCALAPPDATA%` because portable Chromium
/// builds failed to start from there in the Engine feasibility prototype (#24).
#[derive(Debug, Clone)]
pub struct Home {
    engines: PathBuf,
    state: PathBuf,
}

impl Home {
    pub fn from_env() -> Home {
        let root = std::env::var_os("TIZEN_SIM_HOME")
            .map(PathBuf::from)
            .or_else(|| dirs::home_dir().map(|h| h.join(".tizen-sim")))
            .unwrap_or_else(|| PathBuf::from(".tizen-sim"));
        Home::at(root)
    }

    pub fn at(root: impl AsRef<Path>) -> Home {
        let root = root.as_ref();
        Home {
            engines: root.join("engines"),
            state: root.join("state"),
        }
    }

    /// Keeps the Engine cache but puts per-App state somewhere else.
    pub fn with_state_dir(self, state: PathBuf) -> Home {
        Home { state, ..self }
    }

    /// The Engine cache.
    pub fn engine_store(&self) -> EngineStore {
        EngineStore::new(self.engines.clone())
    }

    pub fn engines_dir(&self) -> &Path {
        &self.engines
    }

    pub fn state_dir(&self) -> &Path {
        &self.state
    }
}

/// An App id made safe to use as a file or directory name.
pub(crate) fn app_file_name(app_id: &str) -> String {
    app_id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect()
}
