//! Win32: the console's Ctrl+C, the environment, the registry, job objects and elevation (UAC).

mod elevation;
mod job;
mod registry;

use std::ffi::OsStr;

pub use elevation::{ElevatedProcess, in_hyperv_admins, is_elevated, launch_elevated};
pub use job::{Job, resume_threads_of, terminate};
pub use registry::{Hive, delete_user_registry_value, registry_key_exists, registry_string, registry_value_exists, set_user_registry_string};

/// Sets this process's `PATH`, which the tools it starts inherit; at any time.
pub fn set_path(path: &OsStr) {
    // SAFETY: on Windows `set_var` is sound from any thread (std documents it as "always safe to
    // call on Windows"): the environment block is guarded by the OS and std's lock. The data
    // race that makes it unsafe is libc's getenv, on Unix only.
    unsafe { std::env::set_var("PATH", path) };
}

/// Sends Ctrl+C to every process on `pid`'s console (the program and what it started there, e.g.
/// Terraform and its providers). Whether it was sent. Attaching to another console changes this
/// process's own console state: run it in a throwaway process (see `platform::console`).
pub fn send_ctrl_c(pid: u32) -> bool {
    use windows_sys::Win32::System::Console::{AttachConsole, CTRL_C_EVENT, FreeConsole, GenerateConsoleCtrlEvent, SetConsoleCtrlHandler};
    // SAFETY: plain Win32 calls on this process's console; none takes pointers.
    unsafe {
        // A process attaches to one console at most: leave ours (if any) first.
        FreeConsole();
        if AttachConsole(pid) == 0 {
            return false;
        }
        // The event reaches every process on the console, this one included: ignore it here.
        SetConsoleCtrlHandler(None, 1);
        let sent = GenerateConsoleCtrlEvent(CTRL_C_EVENT, 0) != 0;
        // Delivery is asynchronous: stay attached a moment, so the console outlives the event.
        std::thread::sleep(std::time::Duration::from_millis(200));
        FreeConsole();
        sent
    }
}

/// Takes Ctrl+C again (`SetConsoleCtrlHandler(NULL, FALSE)`), which the console programs this
/// process starts from now on inherit. A process created in a process group of its own starts with
/// Ctrl+C ignored, and so would every program it starts. Whether it was set.
pub fn take_ctrl_c() -> bool {
    use windows_sys::Win32::System::Console::SetConsoleCtrlHandler;
    // SAFETY: no pointer: a null handler routine (this process's own Ctrl+C setting) and a flag.
    unsafe { SetConsoleCtrlHandler(None, 0) != 0 }
}

/// `s` as a NUL-terminated UTF-16 string, for the `W` calls.
fn wide(s: &OsStr) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    s.encode_wide().chain(std::iter::once(0)).collect()
}
