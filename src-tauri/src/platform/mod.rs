//! OS plumbing: the PATH a GUI launch is missing, and one-click installs of dependencies.

use std::path::PathBuf;

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

/// Homebrew, where its installer puts it (Apple silicon, then Intel).
#[cfg(target_os = "macos")]
pub fn brew_bin() -> Option<String> {
    ["/opt/homebrew/bin/brew", "/usr/local/bin/brew"].into_iter().find(|p| std::path::Path::new(p).exists()).map(String::from)
}
