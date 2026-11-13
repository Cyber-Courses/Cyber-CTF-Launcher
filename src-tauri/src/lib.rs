//! The Cyber CTF launcher: a Tauri app that installs, runs and stops CTF labs on the player's
//! machine, servers and cloud accounts. `run` starts it, as the app or as a deploy worker.

mod account;
mod app;
mod cloud;
mod commands;
mod error;
#[cfg(test)]
mod integration_tests;
mod labs;
mod machine;
mod platform;
mod runtime;
mod screen;
mod shared_folder;
mod terminal;
#[cfg(test)]
mod testing;
mod version;

/// The Tauri runtime the app runs on: the real webview, or Tauri's mock in unit tests, so code
/// that takes an app handle (paths under the app data folder, the commands) runs there too.
#[cfg(not(test))]
pub(crate) type AppRuntime = tauri::Wry;
#[cfg(test)]
pub(crate) type AppRuntime = tauri::test::MockRuntime;
/// The app's handle, on the runtime it runs on.
pub(crate) type AppHandle = tauri::AppHandle<AppRuntime>;
/// A webview window, on the runtime the app runs on.
pub(crate) type WebviewWindow = tauri::WebviewWindow<AppRuntime>;
/// Starts the launcher: the app with its windows, or, when started as `deploy --job <file>`, a
/// headless deploy worker for one lab.
///
/// # Errors
///
/// When Tauri can't build the app (its embedded configuration is broken).
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() -> tauri::Result<()> {
    // rustls's crypto provider, before any TLS or random use: ring. The default, AWS-LC, seeds
    // its generator from CPU jitter and aborts the process when the jitter source fails its
    // health test (aws-lc-sys 0.46, tree_jitter_initialize_once: abort(), no message): on a
    // nested Windows 11 VM about one launch in five ended with 0xC0000409, the dumps all on that
    // path. A second install (a test harness's) is no error.
    let _ = rustls::crypto::ring::default_provider().install_default();
    // Before any tool starts: none holds what started the app (#299).
    #[cfg(windows)]
    platform::sys::keep_std_handles();
    app::run()
}
