//! The one place `unsafe` is allowed outside `runtime/`: every call the launcher makes into libc
//! or Win32, each behind a safe function whose `SAFETY:` comment says why the call is sound. It
//! is reviewed as a whole; nothing else in the app shell or the domain uses `unsafe`.

#![expect(unsafe_code, reason = "the audited home of the launcher's calls into libc and Win32")]

#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

use std::ffi::OsStr;

#[cfg(target_os = "macos")]
pub use unix::{boot_session, process_start};
#[cfg(unix)]
pub use unix::{is_root, login_name, process_exists, signal_group, signal_process};
#[cfg(all(windows, test))]
pub use windows::terminate;
#[cfg(windows)]
pub use windows::{
    Created, ElevatedProcess, Hive, Job, create_in_job, delete_user_registry_value, exit_code, in_hyperv_admins, is_elevated, keep_std_handles,
    launch_elevated, process_created, registry_key_exists, registry_string, registry_value_exists, registry_values, send_ctrl_c, set_path,
    set_user_registry_string, take_ctrl_c, terminate_process,
};

/// A registry value as the launcher reads it: a `REG_DWORD`'s number, or the type of any other.
/// Here, not with the Windows calls, so what reads it (the policy) is tested on every OS.
#[cfg(any(windows, test))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegistryValue {
    /// A `REG_DWORD`, with its number.
    Dword(u32),
    /// Any other type, with its type code (`REG_SZ` is 1).
    Other(u32),
}

/// Proof that this process is still single-threaded, which changing its environment needs
/// ([`set_env_at_startup`]): on Unix, another thread reading the environment meanwhile (libc's
/// `getenv`) would be a data race. Only the start of `app::run` makes one, and it is neither
/// `Send` nor `Clone`, so it can't outlive the start or reach another thread.
pub struct SingleThreaded(std::marker::PhantomData<*const ()>);

impl SingleThreaded {
    /// The proof, taken on the caller's word.
    ///
    /// # Safety
    ///
    /// No other thread of this process exists, and none is started while the token lives: the
    /// first lines of `app::run`, or a test run with `--test-threads=1`.
    pub unsafe fn assume() -> SingleThreaded {
        SingleThreaded(std::marker::PhantomData)
    }
}

/// Sets this process's environment variable `name` (`PATH`, a proxy), which the tools it starts
/// inherit; `_only` proves no other thread can read the environment meanwhile.
pub fn set_env_at_startup(_only: &SingleThreaded, name: &str, value: &OsStr) {
    // SAFETY: a `SingleThreaded` exists only while no other thread does (its `assume`'s
    // contract), so nothing reads the environment while it changes.
    unsafe { std::env::set_var(name, value) };
}
