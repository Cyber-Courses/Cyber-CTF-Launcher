//! The app menu. macOS gets an explicit one: Tauri's default labels the About/Hide/Quit items
//! with the crate name (cyberctf-desktop), so they read "Cyber CTF" instead. Edit + Window are
//! kept so clipboard shortcuts and window controls still work.

use crate::AppHandle;
use crate::AppRuntime as Wry;
use tauri::menu::{Menu, MenuEvent};

/// The menu item that opens Settings.
const SETTINGS: &str = "settings";

#[cfg(target_os = "macos")]
pub fn build(handle: &AppHandle) -> tauri::Result<Menu<Wry>> {
    use tauri::menu::{AboutMetadata, MenuBuilder, MenuItem, PredefinedMenuItem, SubmenuBuilder};
    let about = AboutMetadata { name: Some("Cyber CTF".into()), version: Some(env!("CARGO_PKG_VERSION").into()), ..Default::default() };
    // Standard Preferences slot: Cmd+, opens Settings in its own window (see `on_event`).
    let settings = MenuItem::with_id(handle, SETTINGS, "Settings…", true, Some("CmdOrCtrl+,"))?;
    let app_menu = SubmenuBuilder::new(handle, "Cyber CTF")
        .item(&PredefinedMenuItem::about(handle, Some("About Cyber CTF"), Some(about))?)
        .separator()
        .item(&settings)
        .separator()
        .services()
        .separator()
        .item(&PredefinedMenuItem::hide(handle, Some("Hide Cyber CTF"))?)
        .hide_others()
        .show_all()
        .separator()
        .item(&PredefinedMenuItem::quit(handle, Some("Quit Cyber CTF"))?)
        .build()?;
    let edit_menu = SubmenuBuilder::new(handle, "Edit").undo().redo().separator().cut().copy().paste().select_all().build()?;
    let window_menu = SubmenuBuilder::new(handle, "Window").minimize().separator().close_window().build()?;
    MenuBuilder::new(handle).items(&[&app_menu, &edit_menu, &window_menu]).build()
}

/// Linux and Windows: no menu bar. The default one drew a light strip above the dark UI and
/// pushed the webview out of the window; Ctrl+, opens Settings from the page.
#[cfg(not(target_os = "macos"))]
pub fn build(handle: &AppHandle) -> tauri::Result<Menu<Wry>> {
    Menu::new(handle)
}

/// App-menu clicks: "Settings…" opens the settings window.
pub fn on_event(app: &AppHandle, event: &MenuEvent) {
    if event.id().as_ref() == SETTINGS {
        let _ = crate::app::window::open_settings_window(app);
    }
}
