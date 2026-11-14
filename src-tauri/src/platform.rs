//! OS plumbing: the app's folders, the wall clock as Unix time, removing what may already be
//! gone, running tools, files only this user can read, whole-file writes, the service endpoints,
//! HTTP clients with limits, a process's identity the clock can't move, the PATH and the proxy a GUI launch is missing, the console's Ctrl+C
//! on Windows, the files a Store build saved before, the administrator's policy, the pieces of a
//! root script that checks what it downloads, and a future's panic caught where it is polled.

use std::path::{Path, PathBuf};

use crate::error::{Error, Result};

pub mod atomic_file;
pub mod clock;
pub mod config;
pub mod console;
pub mod elevation;
pub mod env_path;
pub mod exec;
pub mod fs;
pub mod host;
pub mod http;
pub mod identity;
pub mod keychain;
pub mod loopback_http;
#[cfg(any(windows, test))]
pub mod msix_data;
pub mod policy;
pub mod private_fs;
pub mod proxy;
pub mod redact;
pub mod remembered;
pub mod root_script;
pub mod sync;
pub mod sys;
pub mod test_root;
pub mod tree_link;
pub mod unwind;

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

    /// The app's folders as Tauri resolves them for the app ([`APP_IDENTIFIER`]), worked out
    /// without Tauri: what a process of the app's that runs without it (the command line, a
    /// deploy worker started by it) shares with the app. An end-to-end run's app keeps them in
    /// its own folder ([`test_root`]).
    pub fn standard() -> AppDirs {
        if let Some(root) = test_root::root() {
            let app = test_root::app_dir(&root);
            return AppDirs::new(Some(app.clone()), Some(app.join("caches")));
        }
        AppDirs::new(env_path::app_data_dir(APP_IDENTIFIER), env_path::app_cache_dir(APP_IDENTIFIER))
    }
}

/// The app's identifier (`identifier` in tauri.conf.json, which a test holds it to): the name of
/// its folders under the OS's data and cache folders.
pub const APP_IDENTIFIER: &str = "org.cyberctf.desktop";

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

/// Whether a program of that name is on this process's PATH (a file there; Windows' extensions
/// aren't tried: the callers name Linux programs).
pub fn on_path(program: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|path| std::env::split_paths(&path).any(|dir| dir.join(program).is_file()))
}

/// `f`, a call that blocks (the keychain, a file tree, a generator), run on the async runtime's
/// blocking threads so it holds up no task; a panic in it is an error, not a crash.
pub async fn off_runtime<T: Send + 'static>(f: impl FnOnce() -> Result<T> + Send + 'static) -> Result<T> {
    tokio::task::spawn_blocking(f).await.map_err(|e| Error::Invalid(e.to_string()))?
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_identifier_is_the_apps() {
        let config: serde_json::Value = serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        assert_eq!(config["identifier"], super::APP_IDENTIFIER);
    }

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
