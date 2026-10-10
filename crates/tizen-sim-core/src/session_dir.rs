//! A Session's own directory under the per-user state, where its Package is
//! unpacked and its Session Registry entry is kept.
//!
//! A lock file, held for the Session's life, marks the directory as in use.
//! The operating system releases the lock when the owner dies, however it
//! dies, so a held lock means a live Session. A Session that ends normally
//! removes its directory; one that is killed can't, so each new Session, and
//! each look at the Registry, first removes directories whose lock is free.

use crate::home::Home;
use std::fs::{File, TryLockError};
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

pub const LOCK_FILE: &str = ".lock";

/// How long a new Session may take between creating its lock file and
/// locking it. An unlocked lock file younger than this may be that Session's,
/// so it is not taken for abandoned.
const LOCKING_GRACE: Duration = Duration::from_secs(10);

/// Where the Session directories, and so the Session Registry, are.
pub fn sessions_dir(home: &Home) -> PathBuf {
    home.state_dir().join("sessions")
}

pub struct SessionDir {
    path: PathBuf,
    /// Taken on drop, so the lock is released before the directory goes.
    lock: Option<File>,
}

impl SessionDir {
    /// Creates a new Session directory in `sessions`, after removing the
    /// ones left behind by Sessions that are no longer running.
    pub fn create(sessions: &Path) -> io::Result<SessionDir> {
        remove_abandoned(sessions);
        let mut id = [0u8; 8];
        getrandom::fill(&mut id).map_err(io::Error::other)?;
        let path = sessions.join(hex::encode(id));
        std::fs::create_dir_all(&path)?;
        let lock = File::create(path.join(LOCK_FILE))?;
        lock.lock()?;
        Ok(SessionDir {
            path,
            lock: Some(lock),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The Session's id: its directory's name.
    pub fn id(&self) -> String {
        self.path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default()
    }
}

impl Drop for SessionDir {
    fn drop(&mut self) {
        drop(self.lock.take());
        if let Err(error) = std::fs::remove_dir_all(&self.path) {
            tracing::warn!(%error, path = %self.path.display(), "could not remove the Session directory");
        }
    }
}

/// Removes every Session directory whose lock no running Session holds, and
/// returns the directories of the running Sessions. A directory without a
/// lock file, or with one just created, may be one another Session is still
/// creating, so it is left alone and not listed.
pub fn remove_abandoned(sessions: &Path) -> Vec<PathBuf> {
    let mut live = vec![];
    let Ok(entries) = std::fs::read_dir(sessions) else {
        return live;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(lock) = File::options().write(true).open(path.join(LOCK_FILE)) else {
            continue;
        };
        match lock.try_lock() {
            Ok(()) if just_created(&lock) => {}
            Ok(()) => {
                drop(lock);
                let _ = std::fs::remove_dir_all(&path);
            }
            Err(TryLockError::WouldBlock) => live.push(path),
            Err(TryLockError::Error(error)) => {
                tracing::warn!(%error, path = %path.display(), "could not check a Session directory");
            }
        }
    }
    live
}

fn just_created(lock: &File) -> bool {
    lock.metadata()
        .and_then(|m| m.modified())
        .ok()
        .and_then(|modified| SystemTime::now().duration_since(modified).ok())
        .is_none_or(|age| age < LOCKING_GRACE)
}

/// The lock file of a Session that was killed a while ago.
#[cfg(test)]
pub(crate) fn write_old_lock(dir: &Path) {
    let lock = File::create(dir.join(LOCK_FILE)).unwrap();
    lock.set_modified(SystemTime::now() - 2 * LOCKING_GRACE)
        .unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removes_its_directory_when_dropped() {
        let sessions = tempfile::tempdir().unwrap();
        let dir = SessionDir::create(sessions.path()).unwrap();
        let path = dir.path().to_path_buf();
        std::fs::write(path.join("file"), "x").unwrap();
        drop(dir);
        assert!(!path.exists());
    }

    #[test]
    fn a_new_session_removes_abandoned_directories_but_not_live_ones() {
        let sessions = tempfile::tempdir().unwrap();
        let running = SessionDir::create(sessions.path()).unwrap();
        // A killed Session: its directory and an unlocked lock file remain.
        let abandoned = sessions.path().join("abandoned");
        std::fs::create_dir_all(abandoned.join("package")).unwrap();
        write_old_lock(&abandoned);
        // One still being created, before its lock file exists.
        let creating = sessions.path().join("creating");
        std::fs::create_dir_all(&creating).unwrap();
        // One that has created its lock file but not locked it yet.
        let locking = sessions.path().join("locking");
        std::fs::create_dir_all(&locking).unwrap();
        std::fs::write(locking.join(LOCK_FILE), "").unwrap();

        let new = SessionDir::create(sessions.path()).unwrap();

        assert!(!abandoned.exists());
        assert!(running.path().is_dir());
        assert!(creating.is_dir());
        assert!(locking.is_dir());
        assert!(new.path().is_dir());
    }
}
