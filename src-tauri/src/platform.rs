//! OS plumbing: the app's folders, the wall clock as Unix time, removing what may already be
//! gone, running tools, files only this user can read, whole-file writes, the service endpoints,
//! HTTP clients with limits, the PATH and the proxy a GUI launch is missing, the console's Ctrl+C
//! on Windows, the files a Store build saved before, the administrator's policy, the pieces of a
//! root script that checks what it downloads, and a future's panic caught where it is polled.

use std::path::{Path, PathBuf};

use crate::error::{Error, Result};

pub mod atomic_file;
pub mod clock;
pub mod config;
pub mod console;
pub mod env_path;
pub mod exec;
pub mod fs;
pub mod http;
pub mod keychain;
#[cfg(any(windows, test))]
pub mod msix_data;
pub mod policy;
pub mod private_fs;
pub mod proxy;
pub mod redact;
pub mod remembered;
pub mod root_script;
pub mod sys;
pub mod test_root;
pub mod tree_link;
pub mod unwind;

/// Resets an in-flight flag to `false` when dropped, so a single-flight scan frees the next one
/// whether it returns normally or is cancelled. The flag is borrowed, so one guard serves every
/// such scan (the running-labs probe, the VM global-status probe) without each re-declaring it.
pub struct ResetOnDrop(pub &'static std::sync::atomic::AtomicBool);

impl Drop for ResetOnDrop {
    fn drop(&mut self) {
        self.0.store(false, std::sync::atomic::Ordering::Release);
    }
}

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

/// The home folder the app keeps its own files in (`.cyberctf/`, `CyberCTF/shared`): the user's,
/// or an end-to-end run's ([`test_root`]). The tools' homes (Docker's, Vagrant's, SSH's) are the
/// user's either way: read them through [`home_dir`].
pub fn own_home() -> Option<PathBuf> {
    test_root::root().map(|root| test_root::home(&root)).or_else(home_dir)
}

/// `bytes` as lowercase hex, two digits each.
pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::with_capacity(bytes.len() * 2), |mut out, b| {
        // Writing to a String can't fail.
        let _ = std::fmt::Write::write_fmt(&mut out, format_args!("{b:02x}"));
        out
    })
}

/// `n` random bytes as lowercase hex: ids, tokens, nonces.
pub fn random_hex(n: usize) -> String {
    hex(&(0..n).map(|_| rand::random::<u8>()).collect::<Vec<_>>())
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
pub fn brew_bin() -> Option<&'static str> {
    ["/opt/homebrew/bin/brew", "/usr/local/bin/brew"].into_iter().find(|p| std::path::Path::new(p).exists())
}

/// Each OS's name as the player reads it, by `std::env::consts::OS`'s id for it: the one table the
/// launcher's messages and the webview (`OS_NAMES`, generated) name an OS from.
pub const OS_NAMES: [(&str, &str); 3] = [("macos", "macOS"), ("windows", "Windows"), ("linux", "Linux")];

/// The name of OS `os` ([`OS_NAMES`]); the id itself for any other.
pub fn os_name(os: &str) -> &str {
    OS_NAMES.iter().find(|(id, _)| *id == os).map_or(os, |(_, name)| name)
}

#[cfg(test)]
mod tests {
    #[test]
    fn bytes_read_as_two_lowercase_hex_digits_each() {
        assert_eq!(super::hex(&[0x00, 0x0f, 0xab, 0xff]), "000fabff");
        assert_eq!(super::hex(&[]), "");
        let (a, b) = (super::random_hex(16), super::random_hex(16));
        assert!(a.len() == 32 && a.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()), "{a}");
        assert_ne!(a, b);
        assert_eq!(super::random_hex(3).len(), 6);
    }
}
