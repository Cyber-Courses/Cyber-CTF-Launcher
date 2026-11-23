//! The shared folder's commands: the setting and opening the folder.

use crate::AppHandle;
use crate::error::Result;
use crate::platform::AppDirs;
use crate::shared_folder::{self, SharedFolderInfo};

/// The shared folder setting.
#[tauri::command]
pub fn shared_folder_get(app: AppHandle) -> Result<SharedFolderInfo> {
    shared_folder::shared_folder_get(&AppDirs::from(&app))
}

/// Turns sharing on or off and sets the folder.
#[tauri::command]
pub fn shared_folder_set(app: AppHandle, enabled: bool, path: &str) -> Result<SharedFolderInfo> {
    shared_folder::shared_folder_set(&AppDirs::from(&app), enabled, path)
}

/// Opens the shared folder in the system's file manager.
#[tauri::command]
pub fn shared_folder_open(app: AppHandle) -> Result<()> {
    shared_folder::shared_folder_open(&AppDirs::from(&app), crate::app::open_folder)
}
