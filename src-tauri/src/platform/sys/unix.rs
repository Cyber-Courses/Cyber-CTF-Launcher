//! libc: process liveness, process-group signals, and the password database.

/// Whether process `pid` exists, by signal 0 (nothing is sent): it does when the signal could be
/// sent, or was refused (another user's process). An id that would name a group (0, or past
/// `pid_t` and so negative) never does.
pub fn process_exists(pid: u32) -> bool {
    let Ok(pid) = libc::pid_t::try_from(pid) else { return false };
    if pid <= 0 {
        return false;
    }
    // SAFETY: kill with signal 0 only checks the process exists.
    let sent = unsafe { libc::kill(pid, 0) } == 0;
    sent || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

/// Sends `signal` to the process group `group` leads (one started with `process_group(0)`).
/// Never to group 0 or 1, which would be this process's own group or every process.
pub fn signal_group(group: u32, signal: libc::c_int) {
    let Ok(group) = libc::pid_t::try_from(group) else { return };
    if group <= 1 {
        return;
    }
    // SAFETY: a plain syscall on a process group id; no memory is involved.
    unsafe {
        libc::killpg(group, signal);
    }
}

/// The login name of the user running this process, from the password database.
pub fn login_name() -> Option<String> {
    // SAFETY: getpwuid returns a pointer into static storage (or null), read at once, before any
    // other call could overwrite it.
    unsafe {
        let pw = libc::getpwuid(libc::getuid());
        if pw.is_null() { None } else { std::ffi::CStr::from_ptr((*pw).pw_name).to_str().ok().map(str::to_string) }
    }
}
