//! Process groups of timed and streamed tools, so a timeout, a cancellation or the app's exit
//! kills the whole tree a tool started, not just the tool: its own process group on unix, a job
//! object (which ends every process in it when it is closed) on Windows.

use std::sync::Mutex;

#[cfg(any(unix, windows))]
use crate::platform::sys;
#[cfg(windows)]
use crate::platform::sys::Job;

/// A tool still running: its process id and, on Windows, the job its tree runs in.
struct Live {
    pid: u32,
    /// A streamed tool (a `vagrant up`, a `compose up`, a `terraform apply`), stopped gently on
    /// the app's or worker's exit; a timed probe is just killed.
    #[cfg_attr(not(unix), allow(dead_code, reason = "only unix stops a streamed tool gently, with SIGTERM"))]
    stream: bool,
    #[cfg(windows)]
    job: std::sync::Arc<Job>,
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
            let Some(job) = child.raw_handle().and_then(Job::for_process) else { return GroupGuard(None) };
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
    // The group was created by `process_group(0)` at spawn.
    #[cfg(unix)]
    sys::signal_group(live.pid, libc::SIGKILL);
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
            sys::signal_group(live.pid, libc::SIGTERM);
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
    let resumed = child.id().is_some_and(sys::resume_threads_of);
    if resumed {
        return Ok(());
    }
    let err = std::io::Error::last_os_error();
    if let Some(pid) = child.id() {
        sys::terminate(pid);
    }
    Err(err)
}
