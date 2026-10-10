//! A Session's own directory under the per-user state, where its Package is
//! unpacked.
//!
//! A lock file, held for the Session's life, marks the directory as in use.
//! A Session that ends normally removes its directory; one that is killed
//! can't, so each new Session first removes directories whose lock is free.

use std::fs::{File, TryLockError};
use std::io;
use std::path::{Path, PathBuf};

const LOCK_FILE: &str = ".lock";

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
}

impl Drop for SessionDir {
    fn drop(&mut self) {
        drop(self.lock.take());
        if let Err(error) = std::fs::remove_dir_all(&self.path) {
            tracing::warn!(%error, path = %self.path.display(), "could not remove the Session directory");
        }
    }
}

/// Removes every Session directory whose lock no running Session holds. A
/// directory without a lock file may be one another Session is still
/// creating, so it is left alone.
fn remove_abandoned(sessions: &Path) {
    let Ok(entries) = std::fs::read_dir(sessions) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(lock) = File::options().write(true).open(path.join(LOCK_FILE)) else {
            continue;
        };
        match lock.try_lock() {
            Ok(()) => {
                drop(lock);
                let _ = std::fs::remove_dir_all(&path);
            }
            Err(TryLockError::WouldBlock) => {}
            Err(TryLockError::Error(error)) => {
                tracing::warn!(%error, path = %path.display(), "could not check a Session directory");
            }
        }
    }
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
        let live = SessionDir::create(sessions.path()).unwrap();
        // A killed Session: its directory and an unlocked lock file remain.
        let abandoned = sessions.path().join("abandoned");
        std::fs::create_dir_all(abandoned.join("package")).unwrap();
        std::fs::write(abandoned.join(LOCK_FILE), "").unwrap();
        // One still being created, before its lock file exists.
        let creating = sessions.path().join("creating");
        std::fs::create_dir_all(&creating).unwrap();

        let new = SessionDir::create(sessions.path()).unwrap();

        assert!(!abandoned.exists());
        assert!(live.path().is_dir());
        assert!(creating.is_dir());
        assert!(new.path().is_dir());
    }
}
