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
//! A build that won't keep to the folder it is given (one without `wdio`, which ignores it)
//! refuses to start rather than use the user's folders ([`refusal`]); the runner never starts
//! one anyway: it reads the binary for `BUILD_MARK` first.
//!
//! The app shell points Tauri's folders and the windows there (`app::test_root_config`); the rest
//! of the app asks here. The desktop runner (`tests/e2e/desktop/runner/run.sh`, `run.ps1`) makes the folder
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

/// What a `wdio` build's binary holds, as it is, and no other build's: the desktop runner reads
/// the app's file for it before it starts the app (`tests/e2e/desktop/runner/preflight.mjs`
/// `WDIO_BUILD_MARK`, the same text), so a build without the feature, which would ignore the
/// run's folder and use the user's, is never started by a run. Read in [`root`], so it stays in
/// the binary.
#[cfg(feature = "wdio")]
pub static BUILD_MARK: &str = "cyberctf-wdio-build: keeps an end-to-end run's folder (CYBERCTF_WDIO_ROOT)";

/// The run's folder, when this is a `wdio` build started with one.
pub fn root() -> Option<PathBuf> {
    #[cfg(feature = "wdio")]
    std::hint::black_box(BUILD_MARK);
    root_of(cfg!(feature = "wdio"), std::env::var_os(VAR))
}

/// Why this app must not start: it was given a run's folder it won't keep to (a build without
/// the `wdio` feature, or a folder that isn't absolute), so it would use the user's own folders
/// under an end-to-end run. `None` when `CYBERCTF_WDIO_ROOT` isn't set, as for every player.
pub fn refusal() -> Option<String> {
    refusal_of(cfg!(feature = "wdio"), std::env::var_os(VAR))
}

fn refusal_of(wdio: bool, value: Option<OsString>) -> Option<String> {
    let value = value?;
    if root_of(wdio, Some(value.clone())).is_some() {
        return None;
    }
    Some(if wdio {
        format!("{VAR} is {}, not an absolute folder: the app would use your own folders instead", PathBuf::from(value).display())
    } else {
        format!(
            "{VAR} is set (an end-to-end run's folder), but this build was made without the `wdio` feature and would use your own folders \
             instead: build it with `cargo build --manifest-path src-tauri/Cargo.toml --features wdio`"
        )
    })
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
    fn an_app_given_a_run_folder_it_wont_keep_to_refuses_to_start() {
        let abs = absolute().into_os_string();
        // A player's app: no run's folder, nothing to refuse, whatever the build.
        assert_eq!(refusal_of(false, None), None);
        assert_eq!(refusal_of(true, None), None);
        // A wdio build keeps to an absolute folder.
        assert_eq!(refusal_of(true, Some(abs.clone())), None);
        // A build without wdio would use the user's folders under a run: it refuses, saying how to build.
        let without = refusal_of(false, Some(abs)).unwrap();
        assert!(without.contains("without the `wdio` feature") && without.contains("--features wdio"), "{without}");
        // A relative folder would follow where the app was started from: refused too.
        assert!(refusal_of(true, Some("relative/run".into())).unwrap().contains("not an absolute folder"));
        // These unit tests aren't a wdio build: the runner's process sets no run's folder for them.
        if std::env::var_os(VAR).is_none() {
            assert_eq!(refusal(), None);
        }
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
