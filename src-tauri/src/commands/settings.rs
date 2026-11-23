//! The settings' commands: read them all, and change some.
//! A change is told to every window (`settings-changed`), so each one's copy follows.

use crate::AppHandle;
use crate::app::events;
use crate::error::Result;
use crate::platform::{AppDirs, off_runtime};
use crate::settings::{self, Change, Settings};

/// Every setting.
#[tauri::command]
pub async fn settings_get(app: AppHandle) -> Result<Settings> {
    let dirs = AppDirs::from(&app);
    off_runtime(move || settings::settings_get(&dirs)).await
}

/// Applies `changes`, all or none, and answers the settings after them.
#[tauri::command]
pub async fn settings_change(app: AppHandle, changes: Vec<Change>) -> Result<Settings> {
    let dirs = AppDirs::from(&app);
    let after = off_runtime(move || settings::settings_change(&dirs, &changes)).await?;
    events::emit(&app, events::SETTINGS_CHANGED, after.clone());
    Ok(after)
}
