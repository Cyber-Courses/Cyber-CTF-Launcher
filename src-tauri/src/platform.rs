//! OS plumbing: the app's folders, running tools, files only this user can read, whole-file
//! writes, the service endpoints, HTTP clients with limits, the PATH a GUI launch is missing, the
//! console's Ctrl+C on Windows, and the files a Store build saved before.

use std::path::{Path, PathBuf};

use crate::error::{Error, Result};

pub mod atomic_file;
pub mod config;
pub mod console;
pub mod env_path;
pub mod exec;
pub mod http;
#[cfg(any(windows, test))]
pub mod msix_data;
pub mod private_fs;
pub mod sys;

/// The app's own folders, as the OS names them: its data (labs, records, marks) and its cache
/// (the self-tests' scratch folders). Each is known once the OS said where it is; what can do
/// without one (a mark, a record) skips its file when it isn't. The app shell resolves them.
#[derive(Clone, Debug, Default)]
pub struct AppDirs {
    data: Option<PathBuf>,
    cache: Option<PathBuf>,
}

impl AppDirs {
    /// The folders, each when known.
    pub fn new(data: Option<PathBuf>, cache: Option<PathBuf>) -> AppDirs {
        AppDirs { data, cache }
    }

    /// The data folder, or why there is none.
    pub fn data(&self) -> Result<&Path> {
        self.data.as_deref().ok_or_else(|| Error::Invalid("the app's data folder is unknown".into()))
    }

    /// The file `name` in the data folder, when that is known.
    pub fn data_file(&self, name: &str) -> Option<PathBuf> {
        self.data.as_ref().map(|d| d.join(name))
    }

    /// The cache folder, or why there is none.
    pub fn cache(&self) -> Result<&Path> {
        self.cache.as_deref().ok_or_else(|| Error::Invalid("the app's cache folder is unknown".into()))
    }
}

/// The user's home folder (`USERPROFILE` on Windows, `HOME` elsewhere).
pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from)
}

/// Whether `sh -n` accepts `script` (the install scripts run on macOS and Linux). Windows has no
/// `sh` unless Git's is on PATH: there the syntax check is skipped, the other assertions still run.
#[cfg(test)]
pub(crate) fn sh_parses(script: &str) -> bool {
    match std::process::Command::new("sh").args(["-n", "-c", script]).status() {
        Ok(status) => status.success(),
        Err(e) if cfg!(windows) && e.kind() == std::io::ErrorKind::NotFound => {
            eprintln!("no sh on PATH: syntax check skipped");
            true
        }
        Err(e) => panic!("sh: {e}"),
    }
}

/// Homebrew, where its installer puts it (Apple silicon, then Intel).
#[cfg(target_os = "macos")]
pub fn brew_bin() -> Option<String> {
    ["/opt/homebrew/bin/brew", "/usr/local/bin/brew"].into_iter().find(|p| std::path::Path::new(p).exists()).map(String::from)
}
