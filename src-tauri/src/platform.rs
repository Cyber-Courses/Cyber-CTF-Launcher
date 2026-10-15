//! OS plumbing: running tools, files only this user can read, whole-file writes, HTTP clients
//! with limits, the PATH a GUI launch is missing, the console's Ctrl+C on Windows, and the files a
//! Store build saved before.

use std::path::{Path, PathBuf};

pub mod atomic_file;
pub mod console;
pub mod env_path;
pub mod exec;
pub mod http;
pub mod msix_data;
pub mod private_fs;

/// The user's home folder (`USERPROFILE` on Windows, `HOME` elsewhere).
pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from)
}

/// Opens folder `path` in the system's file manager (through the opener plugin).
pub fn open_folder(path: &Path) -> crate::error::Result<()> {
    tauri_plugin_opener::open_path(path, None::<&str>).map_err(|e| crate::error::Error::Invalid(format!("couldn't open {}: {e}", path.display())))
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
