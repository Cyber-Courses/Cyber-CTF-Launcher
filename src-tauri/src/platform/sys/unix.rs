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
/// Never to group 0 or 1, which would be this process's own group or every process: that is an
/// error. A group that is gone already (`ESRCH`) is no failure; any other refusal (`EPERM`) is.
pub fn signal_group(group: u32, signal: libc::c_int) -> std::io::Result<()> {
    let group = one_id(group, "a process group")?;
    // SAFETY: a plain syscall on a process group id; no memory is involved.
    sent(unsafe { libc::killpg(group, signal) })
}

/// Sends `signal` to process `pid` alone. Never to an id that would name a group (0, 1, or past
/// `pid_t`), as `kill(1)` reads them: that is an error. A process that is gone already (`ESRCH`)
/// is no failure; any other refusal (`EPERM`) is.
pub fn signal_process(pid: u32, signal: libc::c_int) -> std::io::Result<()> {
    let pid = one_id(pid, "a process")?;
    // SAFETY: a plain syscall on a process id; no memory is involved.
    sent(unsafe { libc::kill(pid, signal) })
}

/// `id` as a `pid_t` naming one process or group: not 0, 1, or past `pid_t` (negative).
fn one_id(id: u32, what: &str) -> std::io::Result<libc::pid_t> {
    libc::pid_t::try_from(id)
        .ok()
        .filter(|id| *id > 1)
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, format!("{id} names no single {what}: never signalled")))
}

/// What `kill`/`killpg` answered (`code`): sent, or gone already, else the OS's error.
fn sent(code: libc::c_int) -> std::io::Result<()> {
    if code == 0 {
        return Ok(());
    }
    let e = std::io::Error::last_os_error();
    if e.raw_os_error() == Some(libc::ESRCH) { Ok(()) } else { Err(e) }
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

#[cfg(test)]
mod tests {
    use std::os::unix::process::CommandExt as _;

    /// A process or group that is gone already is no failure (the stop it was for is done); an id
    /// that names more than one process is refused, never signalled.
    #[test]
    fn a_signal_to_what_is_gone_is_sent_and_one_to_many_processes_is_refused() {
        let mut child = std::process::Command::new("sleep").arg("30").process_group(0).spawn().unwrap();
        let pid = child.id();
        super::signal_group(pid, libc::SIGKILL).unwrap();
        child.wait().unwrap();
        super::signal_group(pid, libc::SIGTERM).expect("a group that is gone");
        super::signal_process(pid, libc::SIGTERM).expect("a process that is gone");
        for many in [0, 1, u32::MAX] {
            assert_eq!(super::signal_group(many, 0).unwrap_err().kind(), std::io::ErrorKind::InvalidInput);
            assert_eq!(super::signal_process(many, 0).unwrap_err().kind(), std::io::ErrorKind::InvalidInput);
        }
    }
}
