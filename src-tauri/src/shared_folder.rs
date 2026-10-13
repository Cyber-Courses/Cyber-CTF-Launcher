//! A folder on this computer shared with the attack box: tools, wordlists and notes go in, loot
//! comes out, and it all survives the box being removed with its lab. Chosen in the setup and in
//! Settings (off until then); mounted at `/shared` in the container attack box and in the attack VM.
//! Kept in the app's data folder, so the background deploy worker sees the same choice.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::AppHandle;
use serde::{Deserialize, Serialize};
use tauri::Manager;

use crate::error::{Error, Result};

/// Where it shows up inside the attack box.
pub const MOUNT_POINT: &str = "/shared";

static FILE: OnceLock<PathBuf> = OnceLock::new();

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SharedFolder {
    pub enabled: bool,
    /// The folder on this computer; the default one when unset.
    pub path: Option<String>,
}

/// Points the setting at the app's data folder. Called at start by the app and the deploy worker.
pub fn init(app: &AppHandle) {
    if let Ok(dir) = app.path().app_data_dir() {
        let _ = FILE.set(dir.join("shared-folder.json"));
    }
}

/// The suggested folder: `CyberCTF/shared` in the home folder.
pub fn default_path() -> Option<PathBuf> {
    crate::platform::home_dir().map(|h| h.join("CyberCTF").join("shared"))
}

fn load() -> SharedFolder {
    load_from(FILE.get().map(PathBuf::as_path))
}

/// The setting in `file`; off when there is no file or it doesn't read.
fn load_from(file: Option<&Path>) -> SharedFolder {
    file.and_then(|f| std::fs::read_to_string(f).ok()).and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

/// The folder to mount, when sharing is on: created if missing. None when off or unusable.
pub fn current() -> Option<PathBuf> {
    folder(load())
}

/// `current` for a loaded setting.
fn folder(s: SharedFolder) -> Option<PathBuf> {
    if !s.enabled {
        return None;
    }
    let path = s.path.map(PathBuf::from).or_else(default_path)?;
    std::fs::create_dir_all(&path).ok()?;
    path.is_dir().then_some(path)
}

fn check(path: &Path) -> Result<()> {
    if !path.is_absolute() {
        return Err(Error::Invalid("choose a full folder path".into()));
    }
    // Mounted as `host:container` for Docker and quoted in a Vagrantfile: keep it to plain paths.
    // A `:` past a Windows drive letter would split Docker's `-v` spec and fail the attack box.
    let text = path.to_string_lossy();
    let after_drive = if cfg!(windows) { text.get(2..).unwrap_or_default() } else { text.as_ref() };
    if text.chars().any(|c| matches!(c, '"' | '\n' | '\r')) || after_drive.contains(':') {
        return Err(Error::Invalid("this folder's name has characters that can't be shared; pick another".into()));
    }
    Ok(())
}

#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "SharedFolder")]
pub struct SharedFolderInfo {
    pub enabled: bool,
    pub path: String,
    pub mount_point: &'static str,
}

#[tauri::command]
pub fn shared_folder_get() -> SharedFolderInfo {
    info(load())
}

/// What the Settings page shows for a setting: the chosen folder, else the default one.
fn info(s: SharedFolder) -> SharedFolderInfo {
    let path = s.path.or_else(|| default_path().map(|p| p.display().to_string())).unwrap_or_default();
    SharedFolderInfo { enabled: s.enabled, path, mount_point: MOUNT_POINT }
}

/// Turns sharing on or off and sets the folder (created now, so a wrong path shows at once).
#[tauri::command]
pub fn shared_folder_set(enabled: bool, path: String) -> Result<SharedFolderInfo> {
    save_to(FILE.get().map(PathBuf::as_path), enabled, &path)?;
    Ok(shared_folder_get())
}

/// `shared_folder_set` into the settings file `file` (None until `init`).
fn save_to(file: Option<&Path>, enabled: bool, path: &str) -> Result<()> {
    let p = PathBuf::from(path.trim());
    if enabled {
        check(&p)?;
        std::fs::create_dir_all(&p).map_err(|e| Error::Invalid(format!("couldn't create {}: {e}", p.display())))?;
    }
    let file = file.ok_or_else(|| Error::Invalid("settings aren't ready yet".into()))?;
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let s = SharedFolder { enabled, path: (!path.trim().is_empty()).then(|| p.display().to_string()) };
    crate::atomic_file::write(file, serde_json::to_string_pretty(&s).map_err(|e| Error::Invalid(e.to_string()))?)?;
    Ok(())
}

/// Opens the folder in the system's file manager.
#[tauri::command]
pub fn shared_folder_open() -> Result<()> {
    let path = current().ok_or_else(|| Error::Invalid("folder sharing is off".into()))?;
    let program = if cfg!(target_os = "macos") {
        "open"
    } else if cfg!(windows) {
        "explorer"
    } else {
        "xdg-open"
    };
    std::process::Command::new(program).arg(&path).spawn()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_plain_absolute_paths_are_shared() {
        let (root, home) = if cfg!(windows) { (r"C:\", r"C:\Users\alex") } else { ("/", "/Users/alex") };
        assert!(check(Path::new("relative/dir")).is_err());
        assert!(check(&Path::new(root).join("home").join("a\"b")).is_err());
        assert!(check(&Path::new(home).join("CyberCTF").join("shared")).is_ok());
        assert!(check(&Path::new(home).join("My Tools")).is_ok());
    }

    fn temp(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!("cyberctf-shared-{tag}-{}", rand::random::<u32>()))
    }

    #[test]
    fn the_setting_is_saved_and_read_back() {
        let root = temp("set");
        let file = root.join("data").join("shared-folder.json");
        let folder = root.join("my shared");
        // No file yet, or none set up: sharing is off.
        assert_eq!(load_from(None), SharedFolder::default());
        assert_eq!(load_from(Some(&file)), SharedFolder::default());

        save_to(Some(&file), true, &format!("  {}  ", folder.display())).unwrap();
        assert!(folder.is_dir(), "the folder is created when sharing is turned on");
        let s = load_from(Some(&file));
        assert_eq!(s, SharedFolder { enabled: true, path: Some(folder.display().to_string()) });
        assert_eq!(folder_of(&s), Some(folder.clone()));
        let shown = info(s);
        assert!(shown.enabled);
        assert_eq!((shown.path, shown.mount_point), (folder.display().to_string(), MOUNT_POINT));

        // Off, with no folder: nothing to mount, and the default folder is suggested.
        save_to(Some(&file), false, " ").unwrap();
        let s = load_from(Some(&file));
        assert_eq!(s, SharedFolder { enabled: false, path: None });
        assert_eq!(folder_of(&s), None);
        assert_eq!(info(s).path, default_path().map(|p| p.display().to_string()).unwrap_or_default());

        std::fs::write(&file, "garbage").unwrap();
        assert_eq!(load_from(Some(&file)), SharedFolder::default());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn turning_sharing_on_needs_a_usable_path_and_settings() {
        let root = temp("bad");
        let file = root.join("shared-folder.json");
        assert_eq!(save_to(Some(&file), true, "relative/dir").unwrap_err().to_string(), "choose a full folder path");
        assert!(!file.exists());
        assert_eq!(save_to(None, false, "").unwrap_err().to_string(), "settings aren't ready yet");
        let _ = std::fs::remove_dir_all(root);
    }

    fn folder_of(s: &SharedFolder) -> Option<PathBuf> {
        folder(s.clone())
    }

    /// The commands, over a settings file of this test's own. The setting is process-wide, so
    /// sharing is never turned on here: other tests' attack boxes would mount the folder.
    #[test]
    fn the_commands_save_and_show_the_setting_and_never_open_a_folder_while_off() {
        let root = temp("cmd");
        let file = root.join("shared-folder.json");
        assert!(FILE.set(file.clone()).is_ok(), "only this test points the setting at a file");
        let folder = root.join("loot");
        let shown = shared_folder_set(false, folder.display().to_string()).unwrap();
        assert!(!shown.enabled);
        assert_eq!((shown.path.as_str(), shown.mount_point), (folder.display().to_string().as_str(), MOUNT_POINT));
        assert_eq!(shared_folder_get().path, folder.display().to_string());
        assert!(file.is_file() && !folder.exists(), "off: the folder isn't created");
        assert_eq!(shared_folder_open().unwrap_err().to_string(), "folder sharing is off");
        // A path that can't be shared is refused and the setting kept.
        assert_eq!(shared_folder_set(true, "relative/dir".into()).err().unwrap().to_string(), "choose a full folder path");
        assert_eq!(shared_folder_get().path, folder.display().to_string());
        assert!(current().is_none());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn the_default_is_in_the_home_folder() {
        let p = default_path().unwrap();
        assert!(p.ends_with("CyberCTF/shared"));
    }
}

#[cfg(test)]
mod proptests;
