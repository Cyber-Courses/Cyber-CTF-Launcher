//! The one place `unsafe` is allowed outside `runtime/`: every call the launcher makes into libc
//! or Win32, each behind a safe function whose `SAFETY:` comment says why the call is sound. It
//! is reviewed as a whole; nothing else in the app shell or the domain uses `unsafe`.

#![allow(unsafe_code, reason = "the audited home of the launcher's calls into libc and Win32")]

#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

use std::ffi::OsStr;

#[cfg(unix)]
pub use unix::{login_name, process_exists, signal_group};
#[cfg(windows)]
pub use windows::{
    ElevatedProcess, Hive, Job, delete_user_registry_value, in_hyperv_admins, is_elevated, launch_elevated, registry_key_exists, registry_string,
    registry_value_exists, resume_threads_of, send_ctrl_c, set_path, set_user_registry_string, terminate,
};

/// Sets this process's `PATH`, which the tools it starts inherit. Only while the process is still
/// single-threaded (the first lines of `run()`, or a test run with `--test-threads=1`): on Unix,
/// another thread reading the environment meanwhile (libc's `getenv`) would be a data race.
pub fn set_path_at_startup(path: &OsStr) {
    // SAFETY: called before any other thread exists (see above), so nothing reads the
    // environment while it changes.
    unsafe { std::env::set_var("PATH", path) };
}
