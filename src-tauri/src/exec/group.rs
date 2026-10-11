//! Process groups of timed tools, so a timeout, a cancellation or the app's exit kills the whole
//! tree a tool started, not just the tool: its own process group on unix, a job object (which
//! ends every process in it when it is closed) on Windows.

use std::sync::Mutex;

/// A timed tool still running: its process id and, on Windows, the job its tree runs in.
struct Live {
    pid: u32,
    #[cfg(windows)]
    job: std::sync::Arc<job::Job>,
}

/// Timed tools still running, so they can be ended when the app (or a deploy worker being
/// stopped) exits: an exit doesn't drop their guards, and a leftover `vagrant status` keeps the
/// machine's lock (every later status of that lab then fails as "locked").
static LIVE_GROUPS: Mutex<Vec<Live>> = Mutex::new(Vec::new());

/// Kills a spawned tool's whole process tree when dropped, unless disarmed after it exited.
pub(super) struct GroupGuard(Option<u32>);

impl GroupGuard {
    pub(super) fn new(child: &tokio::process::Child) -> Self {
        let pid = child.id();
        if let Some(p) = pid {
            #[cfg(windows)]
            let Some(job) = child.raw_handle().and_then(job::Job::for_process) else { return GroupGuard(None) };
            if let Ok(mut live) = LIVE_GROUPS.lock() {
                live.push(Live {
                    pid: p,
                    #[cfg(windows)]
                    job: std::sync::Arc::new(job),
                });
            }
        }
        GroupGuard(pid)
    }

    /// The tool exited on its own: leave whatever it started alone.
    pub(super) fn disarm(mut self) {
        if let Some(live) = forget(self.0.take()) {
            #[cfg(windows)]
            live.job.release();
            #[cfg(not(windows))]
            let _ = live;
        }
    }
}

impl Drop for GroupGuard {
    fn drop(&mut self) {
        if let Some(live) = forget(self.0.take()) {
            kill(&live);
        }
    }
}

fn forget(pid: Option<u32>) -> Option<Live> {
    let pid = pid?;
    let mut live = LIVE_GROUPS.lock().ok()?;
    let at = live.iter().position(|l| l.pid == pid)?;
    Some(live.remove(at))
}

fn kill(live: &Live) {
    #[cfg(unix)]
    // SAFETY: plain syscall; the group was created by `process_group(0)` at spawn.
    unsafe {
        libc::killpg(live.pid as libc::pid_t, libc::SIGKILL);
    }
    #[cfg(windows)]
    live.job.kill();
    #[cfg(not(any(unix, windows)))]
    let _ = live;
}

/// Ends every timed tool this process started that is still running, with what it started. For
/// the app's exit, and a deploy worker being stopped (a timed tool is in a group of its own, so
/// a signal to the worker's group misses it).
pub fn kill_live_tools() {
    let groups = LIVE_GROUPS.lock().map(|mut g| std::mem::take(&mut *g)).unwrap_or_default();
    for live in &groups {
        kill(live);
    }
}

#[cfg(windows)]
mod job {
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JobObjectExtendedLimitInformation, SetInformationJobObject, TerminateJobObject,
    };

    /// A job object a tool's process (and every process it starts) runs in.
    pub(super) struct Job(HANDLE);

    // SAFETY: a job handle is a kernel handle, usable from any thread.
    unsafe impl Send for Job {}
    unsafe impl Sync for Job {}

    impl Job {
        /// A job ending its processes when closed, with `process` in it. `None` when Windows
        /// refuses (the tool then runs as before, its tree not tracked).
        pub(super) fn for_process(process: std::os::windows::io::RawHandle) -> Option<Job> {
            // SAFETY: plain Win32 calls on handles owned here (`process` is the live child's).
            unsafe {
                let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
                if job.is_null() {
                    return None;
                }
                let job = Job(job);
                if !job.set_kill_on_close(true) || AssignProcessToJobObject(job.0, process as HANDLE) == 0 {
                    return None;
                }
                Some(job)
            }
        }

        fn set_kill_on_close(&self, on: bool) -> bool {
            let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            info.BasicLimitInformation.LimitFlags = if on { JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE } else { 0 };
            // SAFETY: `info` is a valid, fully initialised struct of the size given.
            unsafe {
                SetInformationJobObject(
                    self.0,
                    JobObjectExtendedLimitInformation,
                    &info as *const _ as *const core::ffi::c_void,
                    std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                ) != 0
            }
        }

        /// Ends every process in the job.
        pub(super) fn kill(&self) {
            // SAFETY: a live job handle.
            unsafe {
                TerminateJobObject(self.0, 1);
            }
        }

        /// The tool is done: closing the job no longer ends what it left running.
        pub(super) fn release(&self) {
            self.set_kill_on_close(false);
        }
    }

    impl Drop for Job {
        fn drop(&mut self) {
            // SAFETY: the handle is ours and closed once.
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
}
