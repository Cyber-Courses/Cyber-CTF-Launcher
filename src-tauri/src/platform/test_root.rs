//! An end-to-end run's own folders. A build with the `wdio` feature (the one the desktop
//! end-to-end drives, never a release) started with `CYBERCTF_WDIO_ROOT` naming an absolute
//! folder keeps everything of its own under it, so a run on a player's or a developer's machine
//! never reads or changes their labs, settings, session or secrets:
//!
//! - `<root>/app`: the app's data, config and local data folders (Tauri's
//!   `appDirectoriesOverride`), its cache in `caches/` and its logs in `logs/`; on Linux and
//!   Windows the webviews keep their data in the local data folder, so there too;
//! - `<root>/home`: what the app keeps in the home folder, the session and the server secrets
//!   (`.cyberctf/`) and the default shared folder (`CyberCTF/shared`), each in a file whatever the
//!   build (a release keeps the first two in the keychain otherwise). The tools' own homes
//!   (Docker's contexts, Vagrant's boxes, SSH keys) stay the user's: the labs run on them;
//! - on macOS, where the system places a webview's data itself, the webviews use a data store of
//!   the run's own ([`webview_store_of`]: one a run, never the user's), so nothing a run's pages
//!   stored (a lab's last run, a spec's hooks) shows in the next run;
//! - nothing registered with the system (the `cyberctf://` scheme), no Store build's files moved.
//!
//! The app shell points Tauri's folders and the windows there (`app::test_root_config`); the rest
//! of the app asks here. The desktop runner (`tests/desktop/run.sh`, `run.ps1`) makes the folder
//! for each run and removes it afterwards.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// The variable naming the run's folder.
pub const VAR: &str = "CYBERCTF_WDIO_ROOT";

/// The data store of the webviews of a run's app on macOS (the store's identifier, 16 bytes):
/// the run's own, from its folder (the first 16 bytes of the path's SHA-256; a run's folder is
/// made fresh for it), apart from the user's and from every other run's: what one run's pages
/// stored never shows in the next (a fresh run said "last run 3 days ago" from a shared store).
pub fn webview_store_of(root: &Path) -> [u8; 16] {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(root.as_os_str().as_encoded_bytes());
    let mut id = [0; 16];
    id.copy_from_slice(&digest[..16]);
    id
}

/// The run's folder, when this is a `wdio` build started with one.
pub fn root() -> Option<PathBuf> {
    root_of(cfg!(feature = "wdio"), std::env::var_os(VAR))
}

/// The folder `value` names, in a `wdio` build only, and only an absolute one: a relative path
/// would follow the folder the app was started from.
fn root_of(wdio: bool, value: Option<OsString>) -> Option<PathBuf> {
    if !wdio {
        return None;
    }
    value.map(PathBuf::from).filter(|p| p.is_absolute())
}

/// The app's data, config and local data folder under the run's `root`.
pub fn app_dir(root: &Path) -> PathBuf {
    root.join("app")
}

/// The home folder of the app's own files under the run's `root`.
pub fn home(root: &Path) -> PathBuf {
    root.join("home")
}

/// The data store a new window's webview uses: the run's, in a run's app; the default otherwise.
pub fn webview_store() -> Option<[u8; 16]> {
    root().map(|root| webview_store_of(&root))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn absolute() -> PathBuf {
        std::env::temp_dir().join(format!("cyberctf-e2e-root-{}", rand::random::<u32>()))
    }

    #[test]
    fn only_a_wdio_build_takes_a_root_and_only_an_absolute_one() {
        let abs = absolute();
        assert_eq!(root_of(false, Some(abs.clone().into_os_string())), None, "a release build never moves its folders");
        assert_eq!(root_of(true, None), None);
        assert_eq!(root_of(true, Some("relative/run".into())), None);
        assert_eq!(root_of(true, Some(abs.clone().into_os_string())), Some(abs));
        // These unit tests aren't a wdio build: the app's folders are the user's whatever is set.
        assert_eq!(root(), None);
        assert_eq!(webview_store(), None);
        let dir = absolute();
        assert_eq!((app_dir(&dir), home(&dir)), (dir.join("app"), dir.join("home")));
    }

    #[test]
    fn each_run_has_a_webview_store_of_its_own() {
        let (a, b) = (absolute(), absolute());
        assert_eq!(webview_store_of(&a), webview_store_of(&a), "the same run keeps its store across its windows");
        assert_ne!(webview_store_of(&a), webview_store_of(&b), "another run's store is another");
        assert_ne!(webview_store_of(&a), [0; 16], "never the nil identifier, which WebKit refuses");
        // The signed-out spec's folder, under the run's, is a run of its own.
        assert_ne!(webview_store_of(&a), webview_store_of(&a.join("signed-out")));
    }
}
