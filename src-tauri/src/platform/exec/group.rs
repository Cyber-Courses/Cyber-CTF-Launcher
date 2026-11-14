//! Process groups of timed and streamed tools, so a timeout, a cancellation or the app's exit
//! kills the whole tree a tool started, not just the tool: its own process group on unix, a job
//! object (which ends every process in it when it is closed) on Windows. Each tool is recorded in
//! the tools' ledger while it runs, for the next start to end it should the app be killed
//! outright ([`super::ledger`]).

use std::sync::Mutex;

#[cfg(unix)]
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
    /// Its record in the tools' ledger, held for its drop: removed when this drops.
    record: Option<super::ledger::Recorded>,
}

/// Tools still running, so they can be ended when the app (or a deploy worker being
/// stopped) exits: an exit doesn't drop their guards, and a leftover `vagrant status` keeps the
/// machine's lock (every later status of that lab then fails as "locked").
static LIVE_GROUPS: Mutex<Vec<Live>> = Mutex::new(Vec::new());

/// Kills a spawned tool's whole process tree when dropped, unless disarmed after it exited.
pub(crate) struct GroupGuard(Option<u32>);

impl GroupGuard {
    /// Tracks the tool `pid` (`stream`: a long run, stopped gently on unix), the leader of a
    /// process group of its own.
    #[cfg(not(windows))]
    pub(super) fn track(pid: Option<u32>, stream: bool) -> Self {
        if let Some(p) = pid {
            // Written before the registry is locked: no file is written under its lock.
            let record = super::ledger::Recorded::tool(p, stream);
            Self::enter(Live { pid: p, stream, record });
        }
        GroupGuard(pid)
    }

    /// Tracks the tool `pid`, created inside `job` (`stream`: a long run).
    #[cfg(windows)]
    pub(super) fn in_job(pid: u32, stream: bool, job: Job) -> Self {
        let record = super::ledger::Recorded::tool(pid, stream);
        Self::enter(Live { pid, stream, job: std::sync::Arc::new(job), record });
        GroupGuard(Some(pid))
    }

    fn enter(tool: Live) {
        if let Ok(mut live) = LIVE_GROUPS.lock() {
            live.push(tool);
        }
    }

    /// The tool exited on its own: leave whatever it started alone.
    pub(crate) fn disarm(mut self) {
        if let Some(live) = forget(self.0.take()) {
            #[cfg(windows)]
            live.job.release();
            // Done with: its record goes.
            drop(live.record);
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

/// Logs a signal to tool group `pid` that couldn't be sent: the tool runs on, past its end.
#[cfg(unix)]
fn signalled(pid: u32, sent: std::io::Result<()>) {
    if let Err(e) = sent {
        log::warn!("couldn't end tool process group {pid}: {e}");
    }
}

fn kill(live: &Live) {
    // The group was created by `process_group(0)` at spawn.
    #[cfg(unix)]
    signalled(live.pid, sys::signal_group(live.pid, libc::SIGKILL));
    #[cfg(windows)]
    live.job.kill();
    #[cfg(not(any(unix, windows)))]
    let _ = live;
}

/// Ends every tool this process started that is still running, with what it started. For the
/// app's exit, and a deploy worker being stopped (each tool is in a group of its own, so a signal
/// to the worker's group misses it). A streamed tool gets SIGTERM on unix, as it did when it
/// shared the worker's group, so Vagrant or Compose can stop what they were doing; its record
/// stays in the ledger, for the next start to end it should it outlive this process.
pub fn kill_live_tools() {
    let groups = LIVE_GROUPS.lock().map(|mut g| std::mem::take(&mut *g)).unwrap_or_default();
    for live in groups {
        #[cfg(unix)]
        if live.stream {
            signalled(live.pid, sys::signal_group(live.pid, libc::SIGTERM));
            if let Some(record) = live.record {
                record.keep();
            }
            continue;
        }
        kill(&live);
    }
}
