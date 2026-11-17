//! The app's own commands: its windows, quitting and restarting, the log folder, the diagnostics
//! bundle, and what this computer's administrator manages.

use crate::AppHandle;
use crate::app::{logging, quit, updates, window};
use crate::error::{Error, Result};
use crate::platform::policy::{self, Key};

/// What is managed outside the app, for the UI to say so: the policy switches this computer's
/// administrator turned on, and where this copy's updates come from.
#[derive(serde::Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct ManagedSettings {
    policy: Vec<Key>,
    updates: updates::Channel,
}

/// Opens the Settings window, or brings it forward. Async, as every command opening a window is:
/// a synchronous one runs on the main thread, and on Windows the new webview then waits for that
/// thread forever and stays white.
#[tauri::command]
pub async fn open_settings(app: AppHandle) -> Result<()> {
    window::open_settings_window(&app).map_err(|e| Error::Invalid(format!("couldn't open Settings: {e}")))
}

/// Lets the operations in flight finish with the windows hidden, then quits.
#[tauri::command]
pub fn linger_quit(app: AppHandle) {
    quit::linger_quit(app);
}

/// Quits now, whatever is in flight.
#[tauri::command]
pub fn force_quit(app: AppHandle) {
    quit::force_quit(&app);
}

/// Relaunches the app, so an installed update applies without the player quitting by hand.
#[tauri::command]
pub fn restart_app(app: AppHandle) {
    app.restart();
}

/// Opens the log folder in the system's file manager.
#[tauri::command]
pub fn open_log_folder(app: AppHandle) -> Result<()> {
    logging::open_log_folder(&app)
}

/// Saves the diagnostics bundle (see `crate::diagnostics`) to `path`, the file the player picked,
/// with `settings`, the window's settings that matter for a report, by name.
#[tauri::command]
pub async fn save_diagnostics(app: AppHandle, path: String, settings: std::collections::BTreeMap<String, String>) -> Result<()> {
    let sources = crate::diagnostics::Sources {
        dirs: crate::platform::AppDirs::from(&app),
        log_dir: logging::log_dir(&app).ok(),
        home: crate::platform::home_dir(),
        settings,
    };
    crate::diagnostics::export(&sources, std::path::Path::new(&path)).await
}

/// What this computer's administrator and package manager decide for the app.
#[tauri::command]
pub fn managed_settings() -> ManagedSettings {
    let policy = policy::current();
    ManagedSettings { policy: policy.on(), updates: updates::channel(policy) }
}

#[cfg(test)]
mod tests {
    use tauri::Manager;

    use super::*;
    use crate::testing::test_app::TestApp;

    #[test]
    fn the_managed_settings_carry_the_policy_and_the_update_channel() {
        let _policy = policy::Override::with(&[policy::Key::DisableUpdater, policy::Key::DisableToolInstalls]);
        let managed = serde_json::to_value(managed_settings()).unwrap();
        assert_eq!(managed, serde_json::json!({ "policy": ["DisableUpdater", "DisableToolInstalls"], "updates": "policy" }));
    }

    #[tokio::test]
    async fn diagnostics_are_saved_where_the_player_picked() {
        let _tools = crate::testing::tools::Tools::new(|_, _| None);
        let t = TestApp::new();
        let out = t.data().join("picked.tar.gz");
        let settings = std::collections::BTreeMap::from([("vmProvider".to_owned(), "qemu".to_owned())]);
        save_diagnostics(t.handle().clone(), out.display().to_string(), settings).await.unwrap();
        let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(std::fs::File::open(&out).unwrap()));
        let names: Vec<String> = archive.entries().unwrap().map(|e| e.unwrap().path().unwrap().display().to_string()).collect();
        assert!(names.contains(&"cyberctf-diagnostics/host-report.json".to_owned()), "{names:?}");
    }

    #[tokio::test]
    async fn settings_open_in_their_own_window() {
        let t = TestApp::new();
        open_settings(t.handle().clone()).await.unwrap();
        assert!(t.handle().get_webview_window("settings").is_some());
    }
}
