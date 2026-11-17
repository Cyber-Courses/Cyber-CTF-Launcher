//! The Cyber CTF launcher: a Tauri app that installs, runs and stops CTF labs on the player's
//! machine, servers and cloud accounts. `run` starts it, as the app or as a home's runner service;
//! `cli_main` is its command line, `cyberctf`, which runs without the app or a window system.

// Tests start their stand-in tools as processes of their own (clippy.toml's disallowed-methods
// holds the shipped code to `platform::exec`).
#![cfg_attr(test, allow(clippy::disallowed_methods, reason = "tests start their stand-in tools directly"))]
// The machine agent's task (a launch, through the runner's operation and its timed tool runs) is a
// future nested deeper than rustc's default 128 levels lets it prove `Send` (E0275 at its spawn,
// with Isothrum 3.0's dev line of 2026-11-15).
#![recursion_limit = "256"]

mod account;
mod app;
mod cli;
mod cloud_cli;
mod commands;
mod diagnostics;
mod english;
mod error;
#[cfg(test)]
mod integration_tests;
mod labs;
mod machine;
mod platform;
mod run_service;
mod runtime;
mod screen;
mod settings;
mod shared_folder;
mod store;
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
/// Starts the launcher: the app with its windows, or, when started as `serve --home <home>`, the
/// headless runner service of a home (`run_service::host`).
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

/// The command line, `cyberctf` (its own executable, `src/bin/cyberctf.rs`): the launcher's labs,
/// machine setup, servers and account through the same code as the app, with no window system
/// needed. Returns the process's exit code: 0 done, 1 failed, 2 a usage error, 3 something to put
/// right first.
#[allow(clippy::must_use_candidate, reason = "ExitCode is must_use itself: clippy 1.100 refuses a second one, which 1.99 still asks for")]
pub fn cli_main() -> std::process::ExitCode {
    cli::main()
}

/// Runs `task` in the background on the app's async runtime (Tauri's; the command line sets its
/// own with [`use_runtime`]): the domain's background work reaches the runtime through here, never
/// through Tauri itself.
pub(crate) fn spawn(task: impl Future<Output = ()> + Send + 'static) {
    // A task an operation spawns tells its steps as the operation does.
    drop(tauri::async_runtime::spawn(runtime::deploy_log::carried(task)));
}

/// Makes `runtime` the one [`spawn`] runs on: the command line's, which has no app.
pub(crate) fn use_runtime(runtime: tokio::runtime::Handle) {
    tauri::async_runtime::set(runtime);
}
