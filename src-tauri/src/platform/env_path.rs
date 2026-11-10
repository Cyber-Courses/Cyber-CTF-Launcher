//! A GUI app on macOS (and some Linux desktops) starts with a minimal PATH
//! (`/usr/bin:/bin:/usr/sbin:/sbin`), not the user's shell PATH. The tools labs need
//! (docker and its credential helper, vagrant, ovftool) live elsewhere, so append the
//! standard install locations once at startup. Existing entries keep priority.
//!
//! The folders of the tools the app places itself (the OVF Tool, under the app data folder) are
//! added too, last and before they exist: a tool placed while the app runs is then found by
//! what it starts without a restart.
//!
//! On Windows the PATH is complete at startup, but it goes stale: an installer (winget, Docker
//! Desktop, VirtualBox) writes the new PATH to the registry, which a running process never
//! rereads. `refresh` merges it back in before each check, so a tool installed from the app is
//! found without restarting it.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

/// Dirs added even before they exist: a tool installed there while the app runs is then found
/// without a restart, since PATH is only set once at startup. On Linux, the per-user gcloud SDK
/// earlier versions installed (gcloud now comes from Google's apt repository, in /usr/bin).
fn future_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    if cfg!(target_os = "linux")
        && let Some(home) = super::home_dir()
    {
        dirs.push(home.join("google-cloud-sdk/bin"));
    }
    // Vagrant's Windows installer puts it under the system drive, not Program Files: added
    // before it exists, so a Vagrant the setup installs is found without a restart.
    if cfg!(windows) {
        let drive = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into());
        dirs.push(PathBuf::from(format!("{drive}\\HashiCorp\\Vagrant\\bin")));
    }
    dirs
}

/// Where the tools live when they are installed, on this OS.
fn extra_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    #[cfg(unix)]
    {
        dirs.extend(["/usr/local/bin", "/usr/local/sbin", "/opt/homebrew/bin", "/opt/homebrew/sbin"].map(PathBuf::from));
        if let Some(home) = super::home_dir() {
            // Docker Desktop's per-user CLI install, and user-level installs.
            dirs.push(home.join(".docker/bin"));
            dirs.push(home.join(".local/bin"));
        }
    }
    #[cfg(target_os = "macos")]
    {
        dirs.push("/Applications/Docker.app/Contents/Resources/bin".into());
        // utmctl, which the running-labs scan asks for UTM's started VMs, ships inside UTM.
        dirs.push("/Applications/UTM.app/Contents/MacOS".into());
        // ovftool, which vagrant-vmware-esxi needs, ships inside VMware Fusion.
        dirs.push("/Applications/VMware Fusion.app/Contents/Library/VMware OVF Tool".into());
        dirs.push("/Applications/VMware Fusion.app/Contents/Library".into());
    }
    if cfg!(windows) {
        dirs.extend(windows_dirs(|name| std::env::var_os(name)));
    }
    dirs
}

/// The Windows tool folders, from the environment `var` reads. None of these installers adds
/// itself to PATH: VirtualBox (`VBoxManage` drives its VMs) says where it went in
/// `VBOX_MSI_INSTALL_PATH`, and VMware Workstation puts `vmrun` under the 32-bit Program Files.
pub(crate) fn windows_dirs(var: impl Fn(&str) -> Option<OsString>) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Some(vbox) = var("VBOX_MSI_INSTALL_PATH") {
        dirs.push(PathBuf::from(vbox));
    }
    if let Some(pf) = var("ProgramFiles").map(PathBuf::from) {
        dirs.push(pf.join("Oracle").join("VirtualBox"));
        dirs.push(pf.join("VMware").join("VMware OVF Tool"));
        dirs.push(pf.join("Vagrant").join("bin"));
        dirs.push(pf.join("Docker").join("Docker").join("resources").join("bin"));
    }
    if let Some(pf86) = var("ProgramFiles(x86)").map(PathBuf::from) {
        dirs.push(pf86.join("VMware").join("VMware Workstation"));
    }
    // ovftool, which vagrant-vmware-esxi needs, ships inside Workstation, in a folder of its own.
    for pf in ["ProgramFiles(x86)", "ProgramFiles"].into_iter().filter_map(&var).map(PathBuf::from) {
        dirs.push(pf.join("VMware").join("VMware Workstation").join("OVFTool"));
    }
    dirs
}

/// Whether two PATH entries name the same folder: on Windows regardless of case and of a
/// trailing separator (`C:\Tools\` and `c:\tools` are one entry).
fn same_dir(a: &Path, b: &Path) -> bool {
    if cfg!(windows) {
        let norm = |p: &Path| p.to_string_lossy().trim_end_matches(['\\', '/']).to_lowercase();
        norm(a) == norm(b)
    } else {
        a == b
    }
}

/// `current` with each of `dirs` appended that it doesn't hold yet; None when nothing was added.
/// Existing entries keep their order, so they keep priority.
fn appended(current: Option<&OsStr>, dirs: impl IntoIterator<Item = PathBuf>) -> Option<OsString> {
    let mut paths: Vec<PathBuf> = current.map(|p| std::env::split_paths(p).collect()).unwrap_or_default();
    let before = paths.len();
    for dir in dirs {
        if !dir.as_os_str().is_empty() && !paths.iter().any(|p| same_dir(p, &dir)) {
            paths.push(dir);
        }
    }
    if paths.len() == before {
        return None;
    }
    std::env::join_paths(paths).ok()
}

/// `current` with every existing extra dir appended (no duplicates), then the app's own tool
/// folders `own`, last: a tool a hypervisor ships (Fusion's ovftool) comes before the app's copy.
fn augmented(current: Option<&OsStr>, own: &[PathBuf]) -> Option<OsString> {
    appended(current, extra_dirs().into_iter().filter(|d| d.is_dir()).chain(future_dirs()).chain(own.iter().cloned()))
}

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

/// Merges the PATH an installer has since written to the registry (machine, then user), and
/// the tool folders that exist now, into this process's PATH. Children inherit it, so a restart
/// from the app keeps it too. Cheap (two registry reads): call before each machine check.
#[cfg(windows)]
pub fn refresh() {
    let registry = [machine_path(), user_path()].into_iter().flatten();
    let dirs: Vec<PathBuf> = registry.flat_map(|p| std::env::split_paths(&p).collect::<Vec<_>>()).chain(extra_dirs()).filter(|d| d.is_dir()).collect();
    if let Some(path) = appended(std::env::var_os("PATH").as_deref(), dirs) {
        log::info!("PATH refreshed from the registry");
        crate::platform::sys::set_path(&path);
    }
}

#[cfg(not(windows))]
pub fn refresh() {}

/// The machine's `Path` in the registry, expanded.
#[cfg(windows)]
fn machine_path() -> Option<OsString> {
    use crate::platform::sys::{Hive, registry_string};
    registry_string(Hive::LocalMachine, r"SYSTEM\CurrentControlSet\Control\Session Manager\Environment", "Path")
}

/// This user's `Path` in the registry, expanded.
#[cfg(windows)]
fn user_path() -> Option<OsString> {
    use crate::platform::sys::{Hive, registry_string};
    registry_string(Hive::CurrentUser, "Environment", "Path")
}

/// Call first thing in `run()`, before any thread is spawned, with the folders of the tools the
/// app places itself (`own`, which may not exist yet).
pub fn augment(own: &[PathBuf]) {
    if let Some(path) = augmented(std::env::var_os("PATH").as_deref(), own) {
        // At the very start of `run()`: no thread is started yet (Tauri, tokio, a plugin).
        crate::platform::sys::set_env_at_startup("PATH", &path);
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
        crate::platform::sys::set_env_at_startup("PATH", OsStr::new("/usr/bin:/bin:/usr/sbin:/sbin"));
        let found = |tool: &str| std::process::Command::new(tool).arg("--version").output().is_ok();
        assert!(!found("docker"), "docker should be missing on the bare Finder PATH");
        augment(&[]);
        for tool in ["docker", "vagrant"] {
            assert!(found(tool), "{tool} not found after augment(); PATH={:?}", std::env::var("PATH"));
        }
        let compose = std::process::Command::new("docker").args(["compose", "version", "--short"]).output().unwrap();
        assert!(compose.status.success(), "docker compose plugin not found");
    }

    fn joined(dirs: &[&str]) -> OsString {
        std::env::join_paths(dirs.iter().map(PathBuf::from)).unwrap()
    }

    #[test]
    fn appending_keeps_existing_entries_first_and_adds_each_new_dir_once() {
        let out = appended(Some(&joined(&["/a", "/b"])), ["/b", "/c", "/c", ""].map(PathBuf::from)).unwrap();
        assert_eq!(std::env::split_paths(&out).collect::<Vec<_>>(), ["/a", "/b", "/c"].map(PathBuf::from));
    }

    #[test]
    fn windows_finds_the_hypervisors_that_never_join_path() {
        // Regression (#218): VirtualBox read as not installed right after its installer ran, and
        // VMware Workstation's vmrun was never found.
        let env = |name: &str| -> Option<OsString> {
            match name {
                "VBOX_MSI_INSTALL_PATH" => Some(r"D:\VBox\".into()),
                "ProgramFiles" => Some(r"C:\Program Files".into()),
                "ProgramFiles(x86)" => Some(r"C:\Program Files (x86)".into()),
                _ => None,
            }
        };
        let dirs = windows_dirs(env);
        let has = |p: &str| dirs.iter().any(|d| d.to_string_lossy().replace('/', r"\") == p);
        assert!(has(r"D:\VBox\"), "{dirs:?}");
        assert!(has(r"C:\Program Files\Oracle\VirtualBox") && has(r"C:\Program Files\Vagrant\bin"), "{dirs:?}");
        assert!(has(r"C:\Program Files (x86)\VMware\VMware Workstation"), "{dirs:?}");
        // ovftool, inside Workstation in a folder of its own.
        assert!(has(r"C:\Program Files (x86)\VMware\VMware Workstation\OVFTool") && has(r"C:\Program Files\VMware\VMware Workstation\OVFTool"), "{dirs:?}");
        // The installer's own answer comes first: a VirtualBox moved elsewhere wins.
        assert_eq!(dirs[0], PathBuf::from(r"D:\VBox\"));
        assert_eq!(windows_dirs(|_| None), [] as [PathBuf; 0]);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_finds_the_tools_that_ship_inside_app_bundles() {
        // Regression: the running-labs scan asks `utmctl` by name, and UTM keeps it inside its
        // bundle, so a Finder-launched app never found it and no UTM lab ever showed as running.
        // Docker's CLI and Fusion's ovftool live inside their bundles the same way.
        let dirs = extra_dirs();
        for bundled in [
            "/Applications/UTM.app/Contents/MacOS",
            "/Applications/Docker.app/Contents/Resources/bin",
            "/Applications/VMware Fusion.app/Contents/Library/VMware OVF Tool",
        ] {
            assert!(dirs.iter().any(|d| d == Path::new(bundled)), "{bundled} missing from {dirs:?}");
        }
    }

    /// The data folder worked out at startup, before Tauri is built, is the one Tauri resolves:
    /// the OVF Tool the setup places there is on the `PATH` the app started with.
    #[test]
    fn the_startup_data_folder_is_the_one_tauri_resolves() {
        let t = crate::testing::test_app::TestApp::new();
        let identifier = &t.handle().config().identifier;
        assert_eq!(app_data_dir(identifier).as_deref(), Some(t.data()));
    }

    /// The app's own tool folders go on `PATH` last, even before they exist: after a hypervisor's
    /// (Fusion's ovftool first, as the machine check finds it), and found by a search the way
    /// Isothrum's ovftool wrapper looks for the real one (the first `ovftool` file on `PATH`)
    /// once a copy is placed there, without a restart.
    #[test]
    fn the_apps_own_tool_folder_goes_last_and_is_found_once_placed() {
        let data = std::env::temp_dir().join(format!("cyberctf-env-path-{}", rand::random::<u32>()));
        let own = data.join("tools").join("ovftool");
        let bare = joined(&["/usr/bin", "/bin"]);
        let path = augmented(Some(&bare), std::slice::from_ref(&own)).unwrap();
        let dirs: Vec<PathBuf> = std::env::split_paths(&path).collect();
        assert_eq!(dirs.last(), Some(&own), "{dirs:?}");
        assert!(!own.exists(), "added before it exists");
        let exe = if cfg!(windows) { "ovftool.exe" } else { "ovftool" };
        std::fs::create_dir_all(&own).unwrap();
        std::fs::write(own.join(exe), "").unwrap();
        let first = std::env::split_paths(&path).map(|d| d.join(exe)).find(|f| f.is_file());
        let fusion = Path::new("/Applications/VMware Fusion.app/Contents/Library/VMware OVF Tool/ovftool");
        assert!(first == Some(own.join(exe)) || first.as_deref() == Some(fusion), "{first:?}");
        if cfg!(target_os = "macos") {
            let fusion_dir = extra_dirs().iter().position(|d| d.as_path() == fusion.parent().unwrap());
            assert!(fusion_dir.is_some(), "Fusion's ovftool folder is a tool folder");
        }
        let _ = std::fs::remove_dir_all(&data);
    }

    #[test]
    fn nothing_new_means_no_change() {
        assert_eq!(appended(Some(&joined(&["/a"])), [PathBuf::from("/a")]), None);
        assert_eq!(appended(None, []), None);
    }

    #[cfg(windows)]
    #[test]
    fn windows_entries_match_regardless_of_case_and_trailing_separator() {
        assert!(same_dir(Path::new(r"C:\Program Files\Vagrant\bin\"), Path::new(r"c:\program files\vagrant\BIN")));
        let current = joined(&[r"C:\Windows\system32", r"C:\Tools\"]);
        assert_eq!(appended(Some(&current), [PathBuf::from(r"c:\tools"), PathBuf::from(r"C:\WINDOWS\System32")]), None);
    }

    /// The registry PATH reads back expanded (the user Path is usually `REG_EXPAND_SZ` with
    /// %USERPROFILE%), and holds System32 on any Windows install.
    #[cfg(windows)]
    #[test]
    fn the_registry_path_reads_back_expanded() {
        let machine = machine_path().expect("the machine PATH is readable");
        let dirs: Vec<PathBuf> = std::env::split_paths(&machine).collect();
        assert!(dirs.iter().any(|d| d.to_string_lossy().to_lowercase().ends_with(r"\system32")), "{dirs:?}");
        for value in [Some(machine), user_path()].into_iter().flatten() {
            assert!(!value.to_string_lossy().contains('%'), "unexpanded: {value:?}");
        }
    }

    // Unix PATH entries.
    #[cfg(unix)]
    #[test]
    fn appends_existing_dirs_once_and_keeps_order() {
        let tmp = std::env::temp_dir();
        let current = std::env::join_paths([PathBuf::from("/usr/bin"), tmp.clone()]).unwrap();
        let out = augmented(Some(&current), &[]).unwrap_or_default();
        let paths: Vec<PathBuf> = std::env::split_paths(&out).collect();
        if !paths.is_empty() {
            assert_eq!(paths[0], PathBuf::from("/usr/bin"));
            assert_eq!(paths.iter().filter(|p| p.as_path() == std::path::Path::new("/usr/bin")).count(), 1);
        }
    }
}
