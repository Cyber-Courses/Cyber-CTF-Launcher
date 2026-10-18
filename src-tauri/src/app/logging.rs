//! The log file: what a player can send when something went wrong. A release build has no
//! console (on Windows its stderr goes nowhere), so without it a crash leaves no trace.
//!
//! Files live in the app's log folder (`%LOCALAPPDATA%\org.cyberctf.desktop\logs` on Windows,
//! `~/Library/Logs/org.cyberctf.desktop` on macOS, `~/.local/share/org.cyberctf.desktop/logs` on
//! Linux): `cyberctf.log` for the app, `deploy-<lab>.log` for each lab's deploy worker.

use std::path::PathBuf;

use tauri::plugin::TauriPlugin;
use tauri::{Manager, Runtime};
use tauri_plugin_log::{RotationStrategy, Target, TargetKind, TimezoneStrategy};

/// A file is rotated past this size; the last few are kept, so the folder stays small.
const MAX_FILE_BYTES: u128 = 2 * 1024 * 1024;
const KEEP_FILES: usize = 4;

/// The app's log file.
pub const APP_LOG: &str = "cyberctf";

/// The log plugin writing to `<log dir>/<file_name>.log`, and to stdout in development.
///
/// One file per writer: Windows can't rename a file another process holds open, so two processes
/// rotating the same file would fail. The app has one instance; a lab has at most one worker
/// (the per-lab lock), so each worker gets its lab's file.
pub fn plugin<R: Runtime>(file_name: &str) -> TauriPlugin<R> {
    let mut builder = tauri_plugin_log::Builder::new()
        .clear_targets()
        .target(Target::new(TargetKind::LogDir { file_name: Some(file_name.to_owned()) }))
        .level(log::LevelFilter::Info)
        .max_file_size(MAX_FILE_BYTES)
        .rotation_strategy(RotationStrategy::KeepSome(KEEP_FILES))
        .timezone_strategy(TimezoneStrategy::UseLocal);
    if cfg!(debug_assertions) {
        builder = builder.target(Target::new(TargetKind::Stdout));
    }
    builder.build()
}

/// The folder the log files are in.
pub fn log_dir(app: &crate::AppHandle) -> crate::error::Result<PathBuf> {
    app.path().app_log_dir().map_err(|e| crate::error::Error::Invalid(format!("no log folder: {e}")))
}

/// Opens the log folder in the system's file manager (Settings > About), so a player can send
/// the files to support.
pub fn open_log_folder(app: &crate::AppHandle) -> crate::error::Result<()> {
    let dir = log_dir(app)?;
    std::fs::create_dir_all(&dir)?;
    super::open_folder(&dir)
}

/// The deploy worker's log file for this lab. A lab id that isn't one (a damaged job file)
/// shares a fallback file rather than becoming part of a path.
pub fn worker_log_name(lab_id: &str) -> String {
    if crate::runtime::is_lab_id(lab_id) { format!("deploy-{lab_id}") } else { "deploy-unknown".to_owned() }
}

/// Records every panic in the log file, with where it happened and a backtrace, then runs the
/// default hook (stderr). A panic in an async task doesn't end the app, so without this it would
/// pass unseen; one on the main thread does, and this is its only trace.
pub fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let thread = std::thread::current();
        let backtrace = std::backtrace::Backtrace::force_capture();
        log::error!("panic on thread '{}': {info}\n{backtrace}", thread.name().unwrap_or("<unnamed>"));
        log::logger().flush();
        previous(info);
    }));
}

/// The first lines of a session's log: which build, on what, from where. Enough to tell a Store
/// install from an NSIS one, or an x64 build running under emulation on ARM.
pub fn log_startup(app: &crate::AppHandle, role: &str) {
    let exe = std::env::current_exe().map_or_else(|e| format!("<unknown: {e}>"), |p| p.display().to_string());
    log::info!(
        "Cyber CTF {} {role} starting: {} {} ({}), pid {}, exe {exe}",
        app.package_info().version,
        std::env::consts::OS,
        std::env::consts::ARCH,
        os_version(),
        std::process::id(),
    );
}

fn os_version() -> String {
    sysinfo::System::long_os_version().unwrap_or_else(|| "unknown OS version".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_log_folder_is_the_apps_own() {
        let t = crate::testing::test_app::TestApp::new();
        let dir = log_dir(t.handle()).unwrap();
        assert!(dir.is_absolute(), "{}", dir.display());
    }

    #[test]
    fn a_worker_log_is_named_after_its_lab() {
        assert_eq!(worker_log_name("web-sqli_01"), "deploy-web-sqli_01");
    }

    #[test]
    fn a_damaged_lab_id_never_becomes_part_of_a_path() {
        for bad in ["", "../../evil", "a\\b", "C:", "lab name", "con.txt", &"x".repeat(65)] {
            assert_eq!(worker_log_name(bad), "deploy-unknown", "{bad:?}");
        }
    }
}
