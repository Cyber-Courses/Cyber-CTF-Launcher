//! The Cyber CTF launcher: a Tauri app that installs, runs and stops CTF labs on the player's
//! machine, servers and cloud accounts. `run` starts it, as the app or as a deploy worker.

mod account;
mod app;
mod cloud;
mod commands;
mod deploy_worker;
mod error;
mod labs;
mod lan_relay;
mod machine;
mod platform;
mod runtime;
mod shared_folder;
mod terminal;
#[cfg(test)]
mod testing;

/// The Tauri runtime the app runs on: the real webview, or Tauri's mock in unit tests, so code
/// that takes an app handle (paths under the app data folder, the commands) runs there too.
#[cfg(not(test))]
pub(crate) type AppRuntime = tauri::Wry;
#[cfg(test)]
pub(crate) type AppRuntime = tauri::test::MockRuntime;
pub(crate) type AppHandle = tauri::AppHandle<AppRuntime>;
pub(crate) type WebviewWindow = tauri::WebviewWindow<AppRuntime>;
/// Starts the launcher: the app with its windows, or, when started as `deploy --job <file>`, a
/// headless deploy worker for one lab.
///
/// # Errors
///
/// When Tauri can't build the app (its embedded configuration is broken).
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() -> tauri::Result<()> {
    app::run()
}
