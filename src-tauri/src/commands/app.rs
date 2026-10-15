//! The app's own commands: its windows, quitting and restarting, and the log folder.

use crate::AppHandle;
use crate::app::{logging, quit, window};
use crate::error::Result;

/// Opens the Settings window, or brings it forward.
#[tauri::command]
pub fn open_settings(app: AppHandle) -> std::result::Result<(), String> {
    window::open_settings_window(&app).map_err(|e| e.to_string())
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

#[cfg(test)]
mod tests {
    use tauri::Manager;

    use super::*;
    use crate::testing::test_app::TestApp;

    #[test]
    fn settings_open_in_their_own_window() {
        let t = TestApp::new();
        open_settings(t.handle().clone()).unwrap();
        assert!(t.handle().get_webview_window("settings").is_some());
    }
}
