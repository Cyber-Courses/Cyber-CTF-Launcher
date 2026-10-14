//! OS plumbing: the PATH a GUI launch is missing, and one-click installs of dependencies.

use std::path::{Path, PathBuf};

pub mod console;
pub mod env_path;
mod exit_codes;
pub mod install;
mod scripts;
pub mod steps;
pub mod uninstall;

/// The user's home folder (`USERPROFILE` on Windows, `HOME` elsewhere).
pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from)
}

/// Opens `path` in the system's file manager.
pub fn open_folder(path: &Path) -> std::io::Result<()> {
    let program = if cfg!(target_os = "macos") {
        "open"
    } else if cfg!(windows) {
        "explorer"
    } else {
        "xdg-open"
    };
    std::process::Command::new(program).arg(path).spawn().map(drop)
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
