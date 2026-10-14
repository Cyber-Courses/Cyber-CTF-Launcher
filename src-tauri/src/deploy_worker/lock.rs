//! The per-lab lock across processes: an exclusive OS lock on `deploys/<lab>.lock`, held by the
//! app's own lab operations and by a worker for the whole of its job, so the app and a worker
//! (or two workers) never run two operations over the same lab at once. The OS releases it when
//! its holder exits, so a crashed app or worker never leaves the lab locked.

use std::collections::HashMap;
use std::fs::{File, TryLockError};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

/// How often a wait for the lock looks again.
const RETRY: Duration = Duration::from_millis(250);

/// The lock files this process holds, each with how many holders share it: a worker holds its
/// lab's for the whole job, and the operation it runs takes it again (an OS lock taken twice by
/// one process through two handles would wait on itself).
static HELD: Mutex<Option<HashMap<PathBuf, (File, usize)>>> = Mutex::new(None);

/// A held lab lock; released when the last holder in this process drops it.
#[derive(Debug)]
pub struct LabFileLock(PathBuf);

impl Drop for LabFileLock {
    fn drop(&mut self) {
        let mut held = HELD.lock().unwrap_or_else(|e| e.into_inner());
        let Some(map) = held.as_mut() else { return };
        if let Some((_, n)) = map.get_mut(&self.0) {
            *n -= 1;
            if *n == 0 {
                // Closing the file releases the OS lock.
                map.remove(&self.0);
            }
        }
    }
}

/// The failure of a lab lock that couldn't be taken (its file can't be opened or locked): the
/// operation stops there rather than run unlocked.
pub fn unavailable(e: std::io::Error) -> crate::error::Error {
    crate::error::Error::Invalid(format!("Couldn't lock the lab: {e}"))
}

/// The lock file of `lab` in the workers' folder `dir`.
pub fn path_in(dir: &Path, lab: &str) -> PathBuf {
    dir.join(format!("{lab}.lock"))
}

/// Takes the lock at `path` if no other process (or other handle) holds it; `None` when one
/// does. Shared with a holder in this process.
pub fn try_acquire(path: &Path) -> std::io::Result<Option<LabFileLock>> {
    let mut held = HELD.lock().unwrap_or_else(|e| e.into_inner());
    let map = held.get_or_insert_with(HashMap::new);
    if let Some((_, n)) = map.get_mut(path) {
        *n += 1;
        return Ok(Some(LabFileLock(path.to_path_buf())));
    }
    let file = std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(path)?;
    match file.try_lock() {
        Ok(()) => {
            map.insert(path.to_path_buf(), (file, 1));
            Ok(Some(LabFileLock(path.to_path_buf())))
        }
        Err(TryLockError::WouldBlock) => Ok(None),
        Err(TryLockError::Error(e)) => Err(e),
    }
}

/// Takes the lock at `path`, waiting while another process holds it.
pub async fn acquire(path: &Path) -> std::io::Result<LabFileLock> {
    loop {
        if let Some(lock) = try_acquire(path)? {
            return Ok(lock);
        }
        tokio::time::sleep(RETRY).await;
    }
}

/// Whether an operation holds the lock at `path` right now: one of this process, or another
/// process's.
pub fn held(path: &Path) -> bool {
    if HELD.lock().unwrap_or_else(|e| e.into_inner()).as_ref().is_some_and(|m| m.contains_key(path)) {
        return true;
    }
    let Ok(file) = std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(path) else { return false };
    matches!(file.try_lock(), Err(TryLockError::WouldBlock))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Whether `f` holds within a second. A lock is released when its handle closes, but a
    /// process another test starts meanwhile shares the handle for a moment (until it runs its
    /// program), so a release can take that moment to show.
    pub(crate) fn eventually(mut f: impl FnMut() -> bool) -> bool {
        (0..40).any(|_| {
            f() || {
                std::thread::sleep(Duration::from_millis(25));
                false
            }
        })
    }

    fn lock_file(tag: &str) -> (PathBuf, PathBuf) {
        let dir = crate::deploy_worker::files::tests::temp_dir(tag);
        (path_in(&dir, "lab-1"), dir)
    }

    #[tokio::test]
    async fn a_lock_held_elsewhere_keeps_the_lab_busy_until_released() {
        // Regression: the lab lock was an in-memory mutex, so the app and a deploy worker (two
        // processes) could run two operations over the same lab at once. The lock is now an OS
        // file lock; a handle of its own stands in for the other process here (an OS lock
        // conflicts across handles just as across processes).
        let (path, dir) = lock_file("lock-other");
        let other = std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(&path).unwrap();
        other.lock().unwrap();
        assert!(held(&path));
        assert!(try_acquire(&path).unwrap().is_none());
        assert!(tokio::time::timeout(Duration::from_millis(600), acquire(&path)).await.is_err(), "waits while held");
        // The holder exits (its handle closes): the lock is free.
        drop(other);
        assert!(eventually(|| !held(&path)));
        let mine = tokio::time::timeout(Duration::from_secs(2), acquire(&path)).await.unwrap().unwrap();
        assert!(held(&path));
        drop(mine);
        assert!(eventually(|| !held(&path)));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_holder_in_this_process_shares_it_until_the_last_one_lets_go() {
        let (path, dir) = lock_file("lock-shared");
        let job = try_acquire(&path).unwrap().unwrap();
        let op = try_acquire(&path).unwrap().expect("the worker's own operation takes it again");
        // Another process (another handle) is kept out meanwhile.
        let other = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
        assert!(matches!(other.try_lock(), Err(TryLockError::WouldBlock)));
        drop(op);
        assert!(held(&path));
        assert!(matches!(other.try_lock(), Err(TryLockError::WouldBlock)));
        drop(job);
        assert!(eventually(|| other.try_lock().is_ok()));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_lock_in_a_missing_folder_is_an_error_and_never_held() {
        let path = std::env::temp_dir().join("cyberctf-never-created").join("lab-1.lock");
        assert!(try_acquire(&path).is_err());
        assert!(!held(&path));
    }

    #[test]
    fn many_threads_share_a_lock_of_this_process_and_the_last_one_frees_it() {
        let (path, dir) = lock_file("lock-threads");
        let first = try_acquire(&path).unwrap().unwrap();
        let handles: Vec<_> = (0..8)
            .map(|_| {
                let path = path.clone();
                std::thread::spawn(move || {
                    let lock = try_acquire(&path).unwrap().expect("shared with this process");
                    std::thread::sleep(Duration::from_millis(20));
                    drop(lock);
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
        assert!(held(&path), "the first holder still has it");
        let other = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
        assert!(matches!(other.try_lock(), Err(TryLockError::WouldBlock)));
        drop(first);
        assert!(eventually(|| other.try_lock().is_ok()));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn locks_of_two_labs_are_independent_and_an_empty_lock_file_is_left_behind() {
        let dir = crate::deploy_worker::files::tests::temp_dir("lock-two");
        let (a, b) = (path_in(&dir, "lab-a"), path_in(&dir, "lab-b"));
        let _a = try_acquire(&a).unwrap().unwrap();
        assert!(!held(&b));
        assert!(try_acquire(&b).unwrap().is_some());
        // The lock file is the lock's identity, not state: it stays, empty, and is harmless.
        assert_eq!(std::fs::metadata(&a).unwrap().len(), 0);
        assert!(b.exists());
        std::fs::remove_dir_all(dir).ok();
    }
}
