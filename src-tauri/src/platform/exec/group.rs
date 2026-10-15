//! Process groups of timed and streamed tools, so a timeout, a cancellation or the app's exit
//! kills the whole tree a tool started, not just the tool: its own process group on unix, a job
//! object (which ends every process in it when it is closed) on Windows.

use std::sync::Mutex;

/// A tool still running: its process id and, on Windows, the job its tree runs in.
struct Live {
    pid: u32,
    /// A streamed tool (a `vagrant up`, a `compose up`, a `terraform apply`), stopped gently on
    /// the app's or worker's exit; a timed probe is just killed.
    #[cfg_attr(not(unix), allow(dead_code))]
    stream: bool,
    #[cfg(windows)]
    job: std::sync::Arc<job::Job>,
}

/// Tools still running, so they can be ended when the app (or a deploy worker being
/// stopped) exits: an exit doesn't drop their guards, and a leftover `vagrant status` keeps the
/// machine's lock (every later status of that lab then fails as "locked").
static LIVE_GROUPS: Mutex<Vec<Live>> = Mutex::new(Vec::new());

/// Kills a spawned tool's whole process tree when dropped, unless disarmed after it exited.
pub(crate) struct GroupGuard(Option<u32>);

impl GroupGuard {
    /// For a timed probe.
    pub(super) fn new(child: &tokio::process::Child) -> Self {
        Self::track(child, false)
    }

    /// For a streamed tool.
    pub(super) fn stream(child: &tokio::process::Child) -> Self {
        Self::track(child, true)
    }

    fn track(child: &tokio::process::Child, stream: bool) -> Self {
        let pid = child.id();
        if let Some(p) = pid {
            #[cfg(windows)]
            let Some(job) = child.raw_handle().and_then(job::Job::for_process) else { return GroupGuard(None) };
            if let Ok(mut live) = LIVE_GROUPS.lock() {
                live.push(Live {
                    pid: p,
                    stream,
                    #[cfg(windows)]
                    job: std::sync::Arc::new(job),
                });
            }
        }
        GroupGuard(pid)
    }

    /// The tool exited on its own: leave whatever it started alone.
    pub(crate) fn disarm(mut self) {
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

/// Ends every tool this process started that is still running, with what it started. For the
/// app's exit, and a deploy worker being stopped (each tool is in a group of its own, so a signal
/// to the worker's group misses it). A streamed tool gets SIGTERM on unix, as it did when it
/// shared the worker's group, so Vagrant or Compose can stop what they were doing.
pub fn kill_live_tools() {
    let groups = LIVE_GROUPS.lock().map(|mut g| std::mem::take(&mut *g)).unwrap_or_default();
    for live in &groups {
        #[cfg(unix)]
        if live.stream {
            // SAFETY: plain syscall on a group created by `process_group(0)` at spawn.
            unsafe {
                libc::killpg(live.pid as libc::pid_t, libc::SIGTERM);
            }
            continue;
        }
        kill(live);
    }
}

/// `CREATE_SUSPENDED`: a tool starts paused, so it is in its job before it can start anything.
#[cfg(windows)]
pub(super) const CREATE_SUSPENDED: u32 = 0x0000_0004;

/// Lets a tool created suspended (see [`CREATE_SUSPENDED`]) run: resumes its threads (a new
/// process has just its first one). An error, the tool killed, when none could be resumed:
/// left suspended it would never end.
#[cfg(windows)]
pub(super) fn resume(child: &tokio::process::Child) -> std::io::Result<()> {
    let resumed = child.id().is_some_and(job::resume_threads_of);
    if resumed {
        return Ok(());
    }
    let err = std::io::Error::last_os_error();
    if let Some(pid) = child.id() {
        job::terminate(pid);
    }
    Err(err)
}

#[cfg(windows)]
mod job {
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOBOBJECT_BASIC_LIMIT_INFORMATION,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation, SetInformationJobObject, TerminateJobObject,
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
                if !job.set_kill_on_close(true) || AssignProcessToJobObject(job.0, process) == 0 {
                    return None;
                }
                Some(job)
            }
        }

        fn set_kill_on_close(&self, on: bool) -> bool {
            let info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION {
                BasicLimitInformation: JOBOBJECT_BASIC_LIMIT_INFORMATION {
                    LimitFlags: if on { JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE } else { 0 },
                    ..Default::default()
                },
                ..Default::default()
            };
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

    /// Resumes every thread of process `pid`; whether one was.
    pub(super) fn resume_threads_of(pid: u32) -> bool {
        use windows_sys::Win32::System::Threading::{OpenThread, ResumeThread, THREAD_SUSPEND_RESUME};
        let mut resumed = false;
        // SAFETY: a thread snapshot walked with a correctly sized entry; each thread handle is
        // opened, used and closed here, and the snapshot is closed once done.
        unsafe {
            let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
            if snapshot == INVALID_HANDLE_VALUE {
                return false;
            }
            let mut entry: THREADENTRY32 = std::mem::zeroed();
            entry.dwSize = std::mem::size_of::<THREADENTRY32>() as u32;
            let mut more = Thread32First(snapshot, &mut entry) != 0;
            while more {
                if entry.th32OwnerProcessID == pid {
                    let thread = OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID);
                    if !thread.is_null() {
                        resumed |= ResumeThread(thread) != u32::MAX;
                        CloseHandle(thread);
                    }
                }
                more = Thread32Next(snapshot, &mut entry) != 0;
            }
            CloseHandle(snapshot);
        }
        resumed
    }

    /// Ends process `pid` (one that couldn't be let run).
    pub(super) fn terminate(pid: u32) {
        use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_TERMINATE, TerminateProcess};
        // SAFETY: a process handle opened, used and closed here.
        unsafe {
            let process = OpenProcess(PROCESS_TERMINATE, 0, pid);
            if !process.is_null() {
                TerminateProcess(process, 1);
                CloseHandle(process);
            }
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
