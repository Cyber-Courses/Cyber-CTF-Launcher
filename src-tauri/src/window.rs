//! The app's secondary windows (Settings, machine setup, shells) and what they share: the
//! macOS overlaid title bar, attaching to the main window, and fitting the screen.

use crate::AppHandle;
use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};

/// The main window's label.
pub const MAIN: &str = "main";

/// A window height of `wanted` logical pixels, or as much as the primary screen can show (with
/// room for the menu bar and dock) when that is less, so a tall dialog fits its content on a big
/// display and still opens fully on a small laptop screen, where its body scrolls instead.
pub fn height_fitting(app: &AppHandle, wanted: f64) -> f64 {
    let available = app.primary_monitor().ok().flatten().map(|m| m.size().height as f64 / m.scale_factor() - 80.0);
    fit_height(wanted, available)
}

/// `wanted`, or the screen's `available` height when that is less, never under 480.
fn fit_height(wanted: f64, available: Option<f64>) -> f64 {
    wanted.min(available.unwrap_or(wanted)).max(480.0)
}

/// Focuses the window `label`. False when it isn't open.
pub fn focus_existing(app: &AppHandle, label: &str) -> bool {
    app.get_webview_window(label).map(|w| w.set_focus()).is_some()
}

/// A window with the app's look: on macOS the page draws its own title bar under the traffic
/// lights.
pub fn builder<'a>(app: &'a AppHandle, label: &str, url: impl Into<String>) -> WebviewWindowBuilder<'a, crate::AppRuntime, AppHandle> {
    let builder = WebviewWindowBuilder::new(app, label, WebviewUrl::App(url.into().into()));
    #[cfg(target_os = "macos")]
    let builder = builder.title_bar_style(tauri::TitleBarStyle::Overlay).hidden_title(true);
    builder
}

/// `builder` attached to the main window when it is open (it stays above it and closes with it).
pub fn child_of_main<'a>(
    app: &'a AppHandle,
    builder: WebviewWindowBuilder<'a, crate::AppRuntime, AppHandle>,
) -> tauri::Result<WebviewWindowBuilder<'a, crate::AppRuntime, AppHandle>> {
    match app.get_webview_window(MAIN) {
        Some(main) => builder.parent(&main),
        None => Ok(builder),
    }
}

/// Opens Settings in its own window (label `settings`), focusing it if already open. The window
/// loads the main entry with `?window=settings` so the frontend renders only the settings screen.
pub fn open_settings_window(app: &AppHandle) -> tauri::Result<()> {
    if focus_existing(app, "settings") {
        return Ok(());
    }
    // The root page in Settings mode. Not `index.html?...`: in `tauri dev` that resolves against
    // the Next dev server, which has no `/index.html` route (a 404 page), while the root serves
    // in both dev and the static export.
    let settings = builder(app, "settings", "?window=settings").title("Settings").inner_size(760.0, 640.0).min_inner_size(560.0, 480.0).resizable(true);
    child_of_main(app, settings)?.build()?;
    Ok(())
}

/// Opens the Settings window. Used by the sidebar footer and the app-menu item.
#[tauri::command]
pub fn open_settings(app: AppHandle) -> std::result::Result<(), String> {
    open_settings_window(&app).map_err(|e| e.to_string())
}

/// Brings the main window back (it may be hidden while the app finishes in the background, or
/// minimized).
pub fn reveal_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(MAIN) {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_app::TestApp;

    #[test]
    fn settings_open_once_in_their_own_window() {
        // Without the main window: attaching to it needs a real one (Tauri's mock has none).
        let t = TestApp::new();
        open_settings(t.handle().clone()).unwrap();
        let settings = t.handle().get_webview_window("settings").expect("the Settings window");
        assert!(settings.url().unwrap().as_str().ends_with("?window=settings"), "{}", settings.url().unwrap());
        // Asked again: the open one is focused, no second window.
        open_settings(t.handle().clone()).unwrap();
        assert_eq!(t.handle().webview_windows().len(), 1);
        assert!(focus_existing(t.handle(), "settings") && !focus_existing(t.handle(), "nope"));
    }

    #[test]
    fn a_window_fits_the_screen_but_stays_usable() {
        assert_eq!(fit_height(920.0, None), 920.0);
        assert_eq!(fit_height(920.0, Some(1400.0)), 920.0);
        assert_eq!(fit_height(920.0, Some(700.0)), 700.0);
        assert_eq!(fit_height(920.0, Some(300.0)), 480.0);
    }
}
