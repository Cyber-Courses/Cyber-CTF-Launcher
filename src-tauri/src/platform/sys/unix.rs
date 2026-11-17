//! libc: process-group signals, a process's start (macOS), the password database, and a test
//! binary's limit on open files.

/// Whether process `pid` exists, by signal 0 (nothing is sent): it does when the signal could be
/// sent, or was refused (another user's process). An id that would name a group (0, or past
/// `pid_t` and so negative) never does.
#[cfg(test)]
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
#[cfg(test)]
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

/// When process `pid` started, as the kernel stored it at its fork (seconds and microseconds since
/// the epoch, `proc_bsdinfo`'s `pbi_start_tv*`, the start time `kinfo_proc` reports too). Stored,
/// not computed from the boot time: a clock set later doesn't move it. None for a process gone
/// or not readable.
#[cfg(target_os = "macos")]
pub fn process_start(pid: u32) -> Option<(u64, u64)> {
    let pid = libc::c_int::try_from(pid).ok().filter(|p| *p > 0)?;
    let mut info = std::mem::MaybeUninit::<libc::proc_bsdinfo>::zeroed();
    let size = libc::c_int::try_from(std::mem::size_of::<libc::proc_bsdinfo>()).ok()?;
    // SAFETY: `info` is a writable `proc_bsdinfo` of `size` bytes, which PROC_PIDTBSDINFO fills.
    let read = unsafe { libc::proc_pidinfo(pid, libc::PROC_PIDTBSDINFO, 0, info.as_mut_ptr().cast(), size) };
    if read != size {
        return None;
    }
    // SAFETY: proc_pidinfo wrote the whole struct (it returned its size); it started zeroed.
    let info = unsafe { info.assume_init() };
    Some((info.pbi_start_tvsec, info.pbi_start_tvusec))
}

/// This boot's id (`kern.bootsessionuuid`), new at every boot and unmoved by the clock (unlike
/// `kern.boottime`, which the kernel shifts when the clock is set, to keep the uptime true).
#[cfg(target_os = "macos")]
pub fn boot_session() -> Option<String> {
    let name = c"kern.bootsessionuuid";
    let mut buf = [0u8; 64];
    let mut len = buf.len();
    // SAFETY: a NUL-terminated name; `buf` is writable for `len` bytes, and `len` receives the
    // length written.
    let code = unsafe { libc::sysctlbyname(name.as_ptr(), buf.as_mut_ptr().cast(), &raw mut len, std::ptr::null_mut(), 0) };
    if code != 0 {
        return None;
    }
    let text = buf.get(..len)?;
    std::ffi::CStr::from_bytes_until_nul(text).ok().and_then(|s| s.to_str().ok()).map(str::to_owned)
}

/// How many files a test binary may hold open at once: a run of the unit tests under `cargo test`
/// peaks at 230 to 250 with 16 tests at once (one per core), and macOS starts a process with a soft
/// limit of 256. Four times the peak, and still low enough that a leak of what a test opened (a
/// service, a connection, a store left open for the rest of the run: about 1,200 by the end of a
/// run before #530) fails the run rather than hiding in it. The tests of `testing::service` catch
/// such a leak first.
#[cfg(test)]
const TEST_OPEN_FILES: libc::rlim_t = 1024;

// A test binary raises its soft limit on open files to [`TEST_OPEN_FILES`] (never past the hard
// limit, never lowering it) once, as it loads, before any test runs: in tests only, never in the
// app. A limit that can't be read or raised is left as it is.
#[cfg(test)]
ctor::declarative::ctor! {
    #[ctor(unsafe)]
    fn more_open_files_for_the_tests() {
        let mut limit = libc::rlimit { rlim_cur: 0, rlim_max: 0 };
        // SAFETY: getrlimit writes the one rlimit `limit` points to.
        if unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &raw mut limit) } != 0 || limit.rlim_cur >= TEST_OPEN_FILES {
            return;
        }
        limit.rlim_cur = TEST_OPEN_FILES.min(limit.rlim_max);
        // SAFETY: setrlimit reads the one rlimit `limit` points to; a soft limit up to the hard one
        // needs no privilege, and only this process's limit changes.
        unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &raw const limit) };
    }
}

/// Whether this process runs as root (its effective user is 0).
pub fn is_root() -> bool {
    // SAFETY: geteuid takes nothing, can't fail, and touches no memory of ours.
    unsafe { libc::geteuid() == 0 }
}

#[cfg(test)]
mod tests {
    use std::os::unix::process::CommandExt as _;

    /// The tests run with room for the files they open at once, whatever soft limit they were
    /// started with (macOS's 256).
    #[test]
    fn a_test_binary_may_hold_open_the_files_its_tests_open_at_once() {
        let mut limit = libc::rlimit { rlim_cur: 0, rlim_max: 0 };
        // SAFETY: getrlimit writes the one rlimit `limit` points to.
        assert_eq!(unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &raw mut limit) }, 0);
        assert!(limit.rlim_cur >= super::TEST_OPEN_FILES.min(limit.rlim_max), "{} open files at most", limit.rlim_cur);
    }

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
