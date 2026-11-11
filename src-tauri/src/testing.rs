//! What the unit tests share: an app on Tauri's mock runtime, the property tests' strategies, the
//! scans over the crate's own source, the warnings the code logs, and the TypeScript types written
//! from the command structs.

pub mod logs;
pub mod proptest_support;
pub mod source_scan;
pub mod test_app;
mod ts_bindings;

use std::path::{Path, PathBuf};

/// A fresh folder under the temp dir for one test, named after `tag` (`cyberctf-<tag>-XXXXXX`),
/// removed with what it holds when the returned guard drops: at the end of the test, and when a
/// failed assertion unwinds it too.
pub fn temp_dir(tag: &str) -> TempDir {
    TempDir(tempfile::Builder::new().prefix(&format!("cyberctf-{tag}-")).tempdir().unwrap())
}

/// A test's folder ([`temp_dir`]): a path to use as one, removed when dropped. Keep it bound
/// (`let dir = temp_dir(..)`, not `let _ =`) for as long as the test uses the folder.
#[derive(Debug)]
pub struct TempDir(tempfile::TempDir);

impl std::ops::Deref for TempDir {
    type Target = Path;
    fn deref(&self) -> &Path {
        self.0.path()
    }
}

impl AsRef<Path> for TempDir {
    fn as_ref(&self) -> &Path {
        self.0.path()
    }
}

thread_local! {
    /// This thread's folders ([`thread_dir`]), by tag; removed when the thread ends.
    static THREAD_DIRS: std::cell::RefCell<Vec<(&'static str, TempDir)>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// A folder of the calling thread's own for `tag`: the same one on every call from the thread,
/// removed when the thread ends. A unit test runs on a thread of its own, so the folder is the
/// test's, for code that finds its folder itself rather than being given one (the dev session
/// and secrets files a test build keeps away from the player's).
pub fn thread_dir(tag: &'static str) -> PathBuf {
    THREAD_DIRS.with(|dirs| {
        let mut dirs = dirs.borrow_mut();
        if let Some((_, dir)) = dirs.iter().find(|(t, _)| *t == tag) {
            return dir.to_path_buf();
        }
        let dir = temp_dir(tag);
        let path = dir.to_path_buf();
        dirs.push((tag, dir));
        path
    })
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
        let (path, again, other) = std::thread::spawn(|| (thread_dir("guard-thread"), thread_dir("guard-thread"), thread_dir("guard-other"))).join().unwrap();
        assert_eq!(path, again, "one folder a thread");
        assert_ne!(path, other, "one a tag");
        assert!(!path.exists() && !other.exists(), "removed when the thread ended");
        let here = thread_dir("guard-thread");
        assert_ne!(here, path, "each thread its own");
        assert!(here.is_dir());
    }
}
