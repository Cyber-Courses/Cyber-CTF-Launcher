//! The folders the app keeps its data and cache in, worked out before Tauri is built, and the
//! `PATH` the tools are searched on: isothrum-machine works it out
//! ([`isothrum_machine::host::tool_path`]), this sets it.
//!
//! A GUI app on macOS (and some Linux desktops) starts with a minimal `PATH`, not the user's
//! shell's: the folders the tools labs need are installed in are appended once at startup
//! ([`augment`]), then the folders of the tools the app places itself (the OVF Tool, under the
//! app data folder), last and before they exist, so a tool placed while the app runs is found
//! without a restart. On Windows the `PATH` goes stale as installers write theirs to the registry:
//! [`refresh`] merges it back in before each check.

use std::path::PathBuf;

use isothrum_machine::host::tool_path::{self, Env};

/// The app data folder of the app `identifier`, as Tauri resolves it (`app_data_dir`: the OS's
/// data folder, then the identifier), for startup, before Tauri is built.
pub fn app_data_dir(identifier: &str) -> Option<PathBuf> {
    let data = if cfg!(target_os = "macos") {
        super::home_dir().map(|h| h.join("Library").join("Application Support"))
    } else if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else {
        std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).filter(|d| d.is_absolute()).or_else(|| super::home_dir().map(|h| h.join(".local").join("share")))
    };
    data.filter(|d| !d.as_os_str().is_empty()).map(|d| d.join(identifier))
}

/// The app cache folder of the app `identifier`, as Tauri resolves it (`app_cache_dir`: the OS's
/// cache folder, then the identifier), for a process of the app's that runs without Tauri.
pub fn app_cache_dir(identifier: &str) -> Option<PathBuf> {
    let cache = if cfg!(target_os = "macos") {
        super::home_dir().map(|h| h.join("Library").join("Caches"))
    } else if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
    } else {
        std::env::var_os("XDG_CACHE_HOME").map(PathBuf::from).filter(|d| d.is_absolute()).or_else(|| super::home_dir().map(|h| h.join(".cache")))
    };
    cache.filter(|d| !d.as_os_str().is_empty()).map(|d| d.join(identifier))
}

/// Merges the `PATH` an installer has since written to the registry (machine, then user), and
/// the tool folders that exist now, into this process's `PATH` (Windows; nothing to merge
/// elsewhere). Children inherit it, so a restart from the app keeps it too. Cheap (two registry
/// reads): call before each machine check.
pub async fn refresh() {
    let Some(dirs) = tool_path::refreshed(&crate::runtime::providers::machine(), &Env::here()).await else { return };
    #[cfg(windows)]
    if let Ok(path) = std::env::join_paths(dirs) {
        log::info!("PATH refreshed from the registry");
        crate::platform::sys::set_path(&path);
    }
    #[cfg(not(windows))]
    drop(dirs);
}

/// Puts on `PATH` the folders a GUI launch misses and the folders of the tools the app places
/// itself (`own`, which may not exist yet), while `only` this thread runs.
pub fn augment(only: &crate::platform::sys::SingleThreaded, own: &[PathBuf]) {
    let Some(dirs) = tool_path::augmented(isothrum_runner::this_host(), &Env::here(), own) else { return };
    if let Ok(path) = std::env::join_paths(dirs) {
        crate::platform::sys::set_env_at_startup(only, "PATH", &path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// On this machine, with the PATH a Finder-launched app gets, the tools labs need are
    /// found after `augment()` (opt-in: it changes the process PATH; run single-threaded):
    ///   `cargo test --lib finder_path_finds_tools -- --ignored --test-threads=1`
    #[test]
    #[ignore = "changes the process PATH: run alone, with --test-threads=1"]
    fn finder_path_finds_tools() {
        // SAFETY: run alone, with --test-threads=1 (see the `ignore` reason).
        #[allow(unsafe_code, reason = "this test runs alone, so it is the only thread")]
        let only = unsafe { crate::platform::sys::SingleThreaded::assume() };
        crate::platform::sys::set_env_at_startup(&only, "PATH", std::ffi::OsStr::new("/usr/bin:/bin:/usr/sbin:/sbin"));
        let found = |tool: &str| std::process::Command::new(tool).arg("--version").output().is_ok();
        assert!(!found("docker"), "docker should be missing on the bare Finder PATH");
        augment(&only, &[]);
        for tool in ["docker", "vagrant"] {
            assert!(found(tool), "{tool} not found after augment(); PATH={:?}", std::env::var("PATH"));
        }
        let compose = std::process::Command::new("docker").args(["compose", "version", "--short"]).output().unwrap();
        assert!(compose.status.success(), "docker compose plugin not found");
    }

    /// The data folder worked out at startup, before Tauri is built, is the one Tauri resolves:
    /// the OVF Tool the setup places there is on the `PATH` the app started with.
    #[test]
    fn the_startup_data_folder_is_the_one_tauri_resolves() {
        let t = crate::testing::test_app::TestApp::new();
        let identifier = &t.handle().config().identifier;
        assert_eq!(app_data_dir(identifier).as_deref(), Some(t.data()));
    }
}
