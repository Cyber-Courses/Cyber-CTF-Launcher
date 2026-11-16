//! The settings' commands: read them all, change some, and take over the ones the webview kept.
//! A change is told to every window (`settings-changed`), so each one's copy follows.

use std::collections::BTreeMap;

use crate::AppHandle;
use crate::app::events;
use crate::error::Result;
use crate::platform::{AppDirs, off_runtime};
use crate::settings::{self, Change, Imported, Settings};

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

/// Takes the settings an earlier version kept in the webview's storage (`items`, its keys and
/// their text) into the store, and answers the keys taken for the webview to remove.
#[tauri::command]
pub async fn settings_import_webview(app: AppHandle, items: BTreeMap<String, String>) -> Result<Imported> {
    let dirs = AppDirs::from(&app);
    let imported = off_runtime(move || settings::settings_import_webview(&dirs, &items)).await?;
    if !imported.taken.is_empty() {
        events::emit(&app, events::SETTINGS_CHANGED, imported.settings.clone());
    }
    Ok(imported)
}
