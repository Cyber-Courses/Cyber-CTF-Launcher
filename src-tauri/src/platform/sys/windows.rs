//! Win32: the standard handles, the environment, the registry and elevation (UAC).

mod elevation;
mod registry;

use std::ffi::OsStr;

pub use elevation::is_elevated;
#[cfg(test)]
pub use registry::registry_value_exists;
pub use registry::{Hive, delete_user_registry_value, registry_values, set_user_registry_string};

/// Keeps this process's own standard input, output and error to itself. std starts a child with
/// every inheritable handle of its parent, so each tool held whatever the app was started with:
/// a tool left running as the app died, outside its job (#299), held the pipes of what started
/// the app open, a test harness waiting for their end for good. A tool gets pipes of its own; one
/// given these on purpose (`Stdio::inherit`) still gets them, as std duplicates them for it.
pub fn keep_std_handles() {
    use windows_sys::Win32::Foundation::{HANDLE_FLAG_INHERIT, INVALID_HANDLE_VALUE, SetHandleInformation};
    use windows_sys::Win32::System::Console::{GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE};
    for which in [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
        // SAFETY: plain Win32 calls on this process's own standard handles, none closed here; a
        // missing one (a windowed app started without them) is skipped.
        unsafe {
            let handle = GetStdHandle(which);
            if !handle.is_null() && handle != INVALID_HANDLE_VALUE {
                SetHandleInformation(handle, HANDLE_FLAG_INHERIT, 0);
            }
        }
    }
}

/// Sets this process's `PATH`, which the tools it starts inherit; at any time.
pub fn set_path(path: &OsStr) {
    // SAFETY: on Windows `set_var` is sound from any thread (std documents it as "always safe to
    // call on Windows"): the environment block is guarded by the OS and std's lock. The data
    // race that makes it unsafe is libc's getenv, on Unix only.
    unsafe { std::env::set_var("PATH", path) };
}

/// `s` as a NUL-terminated UTF-16 string, for the `W` calls.
fn wide(s: &OsStr) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    s.encode_wide().chain(std::iter::once(0)).collect()
}
