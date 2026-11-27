//! What the unit tests share: an app on Tauri's mock runtime, the property tests' strategies, the
//! scans over the crate's own source, the warnings the code logs, and the TypeScript types written
//! from the command structs, the way the app's tools reach a test's machine ([`machine_runner`],
//! [`tool_set`]: Isothrum's `FakeProcesses`, set with `runtime::providers::testing::fake`), and a
//! [`Switch`] a test shares with the model of its fake tools.

mod guardrails;
pub mod logs;
pub mod proptest_support;
pub mod runner;
pub mod service;
pub mod source_scan;
pub mod test_app;
mod ts_bindings;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

/// The runner the app's tools start through in a unit test: the test's machine's
/// (`runtime::providers::testing::fake`), never this one's. For the platform layer, which names
/// no layer above it.
pub fn machine_runner() -> Arc<dyn isothrum_runner::ProcessRunner> {
    crate::runtime::providers::machine().runner
}

/// The tool set of the machine this thread's test set, if it set one (`platform::exec::ToolSet`).
pub fn tool_set() -> Option<u64> {
    crate::runtime::providers::testing::tool_set()
}

/// A fresh folder under the temp dir for one test, named after `tag` (`cyberctf-<tag>-XXXXXX`),
/// removed with what it holds when the returned guard drops: at the end of the test, and when a
/// failed assertion unwinds it too.
pub fn temp_dir(tag: &str) -> TempDir {
    TempDir(tempfile::Builder::new().prefix(&format!("cyberctf-{tag}-")).tempdir().unwrap().keep())
}

/// A test's folder ([`temp_dir`]): a path to use as one, removed when dropped ([`remove_dir`]).
/// Keep it bound (`let dir = temp_dir(..)`, not `let _ =`) for as long as the test uses the folder.
#[derive(Debug)]
pub struct TempDir(PathBuf);

impl std::ops::Deref for TempDir {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.0
    }
}

impl AsRef<Path> for TempDir {
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        remove_dir(&self.0);
    }
}

/// The test folders [`remove_dir`] couldn't remove yet, with the thread that dropped each:
/// Windows refuses to remove a file that is still open, and a test's runner service holds its
/// store (`state.db`) open until it has ended, which is after the test's own body (its tasks go
/// with the test's runtime, or once idle).
static PENDING: Mutex<Vec<(std::thread::ThreadId, PathBuf)>> = Mutex::new(Vec::new());

/// How long a test's thread, as it ends, waits for the folders it dropped to go: as long as a
/// runner service may take to end (`service::ENDS_WITHIN`: a stopping one drains its running
/// operations for 40 s), one on this host's tools still answering a call the test made.
const LAST_TRY_WITHIN: std::time::Duration = std::time::Duration::from_secs(60);

thread_local! {
    /// Tries this thread's pending folders once more as the thread ends ([`LastTry`]).
    static LAST_TRY: LastTry = const { LastTry };
}

/// Dropped as its thread ends: a unit test's thread, after its runtime (and every task of it:
/// a runner service's own) has gone, so nothing of the test holds its folders any more. It removes
/// those [`remove_dir`] couldn't, waiting [`LAST_TRY_WITHIN`] at most for a store another thread
/// is still closing: the test's process may exit right after (nextest runs each test in its own).
struct LastTry;

impl Drop for LastTry {
    fn drop(&mut self) {
        let me = std::thread::current().id();
        let started = std::time::Instant::now();
        loop {
            retry_removals();
            let mine = PENDING.lock().unwrap_or_else(PoisonError::into_inner).iter().any(|(t, _)| *t == me);
            if !mine || started.elapsed() >= LAST_TRY_WITHIN {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
}

/// Removes test folder `dir` with what it holds, the launcher's stores in it closed first. One
/// Windows won't remove yet (a file in it still open: a runner service's store) is removed once
/// that service has ended ([`retry_removals`]), or as the test's thread ends ([`LastTry`]),
/// before the test's process exits. Unix removes it at once, open files and all.
pub fn remove_dir(dir: &Path) {
    crate::store::forget_under(dir);
    if std::fs::remove_dir_all(dir).is_err() && dir.exists() {
        PENDING.lock().unwrap_or_else(PoisonError::into_inner).push((std::thread::current().id(), dir.to_path_buf()));
        // A thread that is ending already has no last try to make: the folder waits for another's.
        let _ = LAST_TRY.try_with(|_| ());
    }
}

/// Whether `home` is in a folder a test let go that Windows wouldn't remove yet: for the service
/// serving it, as good as gone.
pub fn let_go(home: &Path) -> bool {
    PENDING.lock().unwrap_or_else(PoisonError::into_inner).iter().any(|(_, dir)| home.starts_with(dir))
}

/// Whether a folder [`remove_dir`] couldn't remove is still waiting to go.
pub fn removals_pending() -> bool {
    !PENDING.lock().unwrap_or_else(PoisonError::into_inner).is_empty()
}

/// Removes again the folders [`remove_dir`] couldn't: those nothing holds any more go. A test's
/// runner service calls it once it has ended (`service`).
pub fn retry_removals() {
    let pending = std::mem::take(&mut *PENDING.lock().unwrap_or_else(PoisonError::into_inner));
    let left: Vec<_> = pending.into_iter().filter(|(_, dir)| std::fs::remove_dir_all(dir).is_err() && dir.exists()).collect();
    PENDING.lock().unwrap_or_else(PoisonError::into_inner).extend(left);
}

thread_local! {
    /// This thread's folders ([`thread_dir`]), by tag; removed when the thread ends.
    static THREAD_DIRS: std::cell::RefCell<Vec<(&'static str, ThreadDir)>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// A thread's folder ([`thread_dir`]): a path no one else has, removed with what it holds when the
/// thread ends, if it was made at all.
#[derive(Debug)]
struct ThreadDir(PathBuf);

impl Drop for ThreadDir {
    fn drop(&mut self) {
        if self.0.exists() {
            remove_dir(&self.0);
        }
    }
}

/// A folder of the calling thread's own for `tag`: the same path on every call from the thread,
/// removed when the thread ends. A unit test runs on a thread of its own, so the folder is the
/// test's, for code that finds its folder itself rather than being given one (the dev session
/// and secrets files a test build keeps away from the player's).
///
/// The folder isn't made: whoever writes in it makes it (as the player's own home folder is
/// there before the app writes in it). A thread that never ends (a worker of the app's shared
/// runtime, which a test's launch reads the session on) only reads, so it leaves nothing behind:
/// a folder made at the first call stayed in the temp folder after every run.
pub fn thread_dir(tag: &'static str) -> PathBuf {
    /// Tells this process's folders apart (with its id, from another process's).
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    THREAD_DIRS.with(|dirs| {
        let mut dirs = dirs.borrow_mut();
        if let Some((_, dir)) = dirs.iter().find(|(t, _)| *t == tag) {
            return dir.0.clone();
        }
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("cyberctf-{tag}-{}-{n}", std::process::id()));
        dirs.push((tag, ThreadDir(path.clone())));
        path
    })
}

/// A switch a test and the model of its fake tools (`FakeProcesses::model`) share: off until one
/// of them turns it on (a daemon started, a sign-in that ran out), which the other then sees.
#[derive(Debug, Clone, Default)]
pub struct Switch(Arc<AtomicBool>);

impl Switch {
    pub fn turn_on(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    pub fn is_on(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

/// Asserts that `actual` is `expected` as a float can be: within a millionth of it (an exact
/// comparison of floats says more about rounding than about the code under test).
#[track_caller]
pub fn assert_close(actual: f64, expected: f64) {
    assert!((actual - expected).abs() < 1e-6, "{actual} is not {expected}");
}

mod tests {
    use super::*;

    #[test]
    fn a_tests_folder_goes_when_it_ends_even_by_a_failed_assertion() {
        // Regression: `temp_dir` left its folder behind (`cyberctf-<tag>-…`), thousands of them
        // in the developer's temp folder after a few hundred runs.
        let dir = temp_dir("guard");
        let path = dir.to_path_buf();
        std::fs::write(dir.join("f"), "x").unwrap();
        assert!(path.file_name().unwrap().to_string_lossy().starts_with("cyberctf-guard-"), "{}", path.display());
        drop(dir);
        assert!(!path.exists(), "removed when dropped");
        let seen = std::sync::Mutex::new(PathBuf::new());
        let failed = std::panic::catch_unwind(|| {
            let dir = temp_dir("guard-panic");
            *seen.lock().unwrap() = dir.to_path_buf();
            panic!("a failed assertion");
        });
        assert!(failed.is_err());
        let path = seen.into_inner().unwrap();
        assert!(path.starts_with(std::env::temp_dir()) && !path.exists(), "removed by the unwinding: {}", path.display());
    }

    #[test]
    fn a_threads_folder_is_the_same_on_each_call_and_goes_with_the_thread() {
        let (path, again, other) = std::thread::spawn(|| {
            let made = thread_dir("guard-thread");
            std::fs::create_dir(&made).unwrap();
            std::fs::write(made.join("f"), "x").unwrap();
            (made, thread_dir("guard-thread"), thread_dir("guard-other"))
        })
        .join()
        .unwrap();
        assert_eq!(path, again, "one folder a thread");
        assert_ne!(path, other, "one a tag");
        assert!(!path.exists() && !other.exists(), "removed when the thread ended");
        let here = thread_dir("guard-thread");
        assert_ne!(here, path, "each thread its own");
        assert!(here.starts_with(std::env::temp_dir()));
    }

    #[test]
    fn a_threads_folder_is_made_by_its_first_writer_never_by_asking_for_it() {
        // Regression: the folder was made at the first call, so a thread that never ends (a
        // worker of the app's runtime, reading the session for a test's launch) left a
        // `cyberctf-test-session-*` folder in the temp folder after every run.
        let path = thread_dir("guard-unmade");
        assert!(!path.exists(), "nothing made for a reader: {}", path.display());
        assert_eq!(thread_dir("guard-unmade"), path);
        assert!(!path.exists());
    }
}
