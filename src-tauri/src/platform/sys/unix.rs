//! libc: the password database, the effective user, and a test binary's limit on open files.

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
    /// The tests run with room for the files they open at once, whatever soft limit they were
    /// started with (macOS's 256).
    #[test]
    fn a_test_binary_may_hold_open_the_files_its_tests_open_at_once() {
        let mut limit = libc::rlimit { rlim_cur: 0, rlim_max: 0 };
        // SAFETY: getrlimit writes the one rlimit `limit` points to.
        assert_eq!(unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &raw mut limit) }, 0);
        assert!(limit.rlim_cur >= super::TEST_OPEN_FILES.min(limit.rlim_max), "{} open files at most", limit.rlim_cur);
    }
}
