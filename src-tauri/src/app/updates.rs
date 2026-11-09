//! How this copy of the app is updated: by its own updater (the release feed on GitHub), or by
//! whatever installed it. A copy a package manager owns (a Homebrew cask, a Debian package, the
//! Microsoft Store) is updated through it: an in-app update would replace files the package
//! manager tracks, and the two would fight over the version. An administrator's policy
//! (`DisableUpdater`) turns the in-app updater off too.
//!
//! When it is off the updater plugin isn't registered at all, so nothing in the webview can check
//! for or install an update; the UI asks [`channel`] to say why instead of offering one.

use std::path::Path;

use crate::platform::policy::{Key, Policy};

/// Where this copy's updates come from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "UpdateChannel")]
pub enum Channel {
    /// The app's own updater.
    InApp,
    /// The organization's policy turned the in-app updater off: IT deploys updates.
    Policy,
    /// A Homebrew cask installed it: `brew upgrade` updates it.
    Homebrew,
    /// A Linux package (a .deb through apt) installed it: the system's package manager updates it.
    LinuxPackage,
    /// The Microsoft Store installed it, and updates it.
    MicrosoftStore,
}

/// This copy's update channel, under `policy`.
pub fn channel(policy: Policy) -> Channel {
    if policy.is_on(Key::DisableUpdater) {
        return Channel::Policy;
    }
    match std::env::current_exe() {
        Ok(exe) => installed_by(&exe).unwrap_or(Channel::InApp),
        Err(e) => {
            log::warn!("couldn't tell how this copy was installed ({e}): updates in the app");
            Channel::InApp
        }
    }
}

/// The package manager that installed the app running from `exe`, if one did: each is looked for
/// on its own OS only.
fn installed_by(exe: &Path) -> Option<Channel> {
    if cfg!(target_os = "macos") && homebrew_cask(exe) {
        Some(Channel::Homebrew)
    } else if cfg!(target_os = "linux") && linux_package(exe, std::env::var_os("APPIMAGE").is_some()) {
        Some(Channel::LinuxPackage)
    } else if cfg!(windows) && store_package(exe) {
        Some(Channel::MicrosoftStore)
    } else {
        None
    }
}

/// Whether a Homebrew cask installed the app bundle `exe` runs from (Homebrew's prefix on Apple
/// Silicon, then on Intel).
fn homebrew_cask(exe: &Path) -> bool {
    let Some(bundle) = app_bundle(exe) else { return false };
    ["/opt/homebrew/Caskroom", "/usr/local/Caskroom"].into_iter().any(|room| cask_installed(Path::new(room), &bundle))
}

/// The name of the `.app` bundle `exe` runs from (`Cyber CTF.app`).
fn app_bundle(exe: &Path) -> Option<String> {
    let is_bundle = |dir: &Path| dir.extension().is_some_and(|e| e.eq_ignore_ascii_case("app"));
    exe.ancestors().filter(|dir| is_bundle(dir)).find_map(Path::file_name).map(|n| n.to_string_lossy().into_owned())
}

/// Whether a cask in the Caskroom `room` installed the app bundle `bundle`: its install receipt
/// (`<token>/.metadata/INSTALL_RECEIPT.json`) lists it among the apps it would remove.
fn cask_installed(room: &Path, bundle: &str) -> bool {
    let Ok(casks) = std::fs::read_dir(room) else { return false };
    casks.flatten().any(|cask| {
        let receipt = cask.path().join(".metadata").join("INSTALL_RECEIPT.json");
        std::fs::read_to_string(receipt).is_ok_and(|text| receipt_lists_app(&text, bundle))
    })
}

/// Whether a cask's install receipt names the app bundle `bundle` among its `app` artifacts.
fn receipt_lists_app(receipt: &str, bundle: &str) -> bool {
    let Ok(receipt) = serde_json::from_str::<serde_json::Value>(receipt) else { return false };
    let artifacts = receipt["uninstall_artifacts"].as_array().map(Vec::as_slice).unwrap_or_default();
    artifacts.iter().filter_map(|a| a["app"].as_array()).flatten().any(|app| app.as_str() == Some(bundle))
}

/// Whether the app running from `exe` came from a Linux package: installed under `/usr` (where
/// a .deb puts it, and only root can write) but not `/usr/local` (a copy installed by hand), and
/// not an `AppImage` (which says so in `APPIMAGE` and runs from a mount under /tmp).
fn linux_package(exe: &Path, appimage: bool) -> bool {
    !appimage && exe.starts_with("/usr") && !exe.starts_with("/usr/local")
}

/// Whether the app running from `exe` is a Microsoft Store (MSIX) package: those are installed
/// under `Program Files\WindowsApps`.
fn store_package(exe: &Path) -> bool {
    // Split by hand: a test on another OS reads a Windows path as one component.
    exe.to_string_lossy().split(['\\', '/']).any(|part| part.eq_ignore_ascii_case("WindowsApps"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::policy;

    #[test]
    fn the_policy_turns_the_in_app_updater_off_whatever_installed_the_app() {
        let _policy = policy::Override::with(&[Key::DisableUpdater]);
        assert_eq!(channel(policy::current()), Channel::Policy);
    }

    #[test]
    fn a_debian_package_updates_through_apt_and_an_appimage_by_itself() {
        assert!(linux_package(Path::new("/usr/bin/cyberctf-desktop"), false));
        assert!(!linux_package(Path::new("/usr/bin/cyberctf-desktop"), true), "an AppImage");
        assert!(!linux_package(Path::new("/tmp/.mount_CyberC/usr/bin/cyberctf-desktop"), false));
        assert!(!linux_package(Path::new("/home/ada/Apps/cyberctf-desktop"), false));
        assert!(!linux_package(Path::new("/usr/local/bin/cyberctf-desktop"), false), "installed by hand");
    }

    #[test]
    fn a_store_package_runs_from_windowsapps() {
        assert!(store_package(Path::new(r"C:\Program Files\WindowsApps\F4K.CyberCTF_1.8.9.0_x64__abc\cyberctf-desktop.exe")));
        assert!(store_package(Path::new("C:/Program Files/windowsapps/F4K.CyberCTF/cyberctf-desktop.exe")));
        assert!(!store_package(Path::new(r"C:\Users\ada\AppData\Local\Cyber CTF\cyberctf-desktop.exe")));
    }

    #[test]
    fn a_homebrew_cask_is_found_by_the_app_its_receipt_lists() {
        assert_eq!(app_bundle(Path::new("/Applications/Cyber CTF.app/Contents/MacOS/cyberctf-desktop")).as_deref(), Some("Cyber CTF.app"));
        assert_eq!(app_bundle(Path::new("/usr/local/bin/cyberctf-desktop")), None);
        let room = std::env::temp_dir().join(format!("cyberctf-caskroom-{}", rand::random::<u64>()));
        let receipt = |token: &str, app: &str| {
            let meta = room.join(token).join(".metadata");
            std::fs::create_dir_all(&meta).unwrap();
            let text = serde_json::json!({ "uninstall_artifacts": [{ "uninstall": [{ "quit": "x" }] }, { "app": [app] }] });
            std::fs::write(meta.join("INSTALL_RECEIPT.json"), text.to_string()).unwrap();
        };
        receipt("flameshot", "flameshot.app");
        assert!(!cask_installed(&room, "Cyber CTF.app"));
        receipt("cyber-ctf", "Cyber CTF.app");
        assert!(cask_installed(&room, "Cyber CTF.app"));
        assert!(!cask_installed(&room.join("missing"), "Cyber CTF.app"));
        assert!(!receipt_lists_app("not json", "Cyber CTF.app"));
        std::fs::remove_dir_all(&room).unwrap();
    }
}
