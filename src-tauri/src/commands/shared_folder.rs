//! The shared folder's commands: the setting and opening the folder.

use crate::error::Result;
use crate::shared_folder::{self, SharedFolderInfo};

/// The shared folder setting.
#[tauri::command]
pub fn shared_folder_get() -> SharedFolderInfo {
    shared_folder::shared_folder_get()
}

/// Turns sharing on or off and sets the folder.
#[tauri::command]
pub fn shared_folder_set(enabled: bool, path: &str) -> Result<SharedFolderInfo> {
    shared_folder::shared_folder_set(enabled, path)
}

/// Opens the shared folder in the system's file manager.
#[tauri::command]
pub fn shared_folder_open() -> Result<()> {
    shared_folder::shared_folder_open()
}
