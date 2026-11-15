//! The tools this process runs, written down under the app's data folder while they run, so the
//! next start ends what a killed app or deploy worker left behind.
//!
//! The app's own exit ends its tools ([`super::kill_live_tools`]); one killed outright (SIGKILL, a
//! crash, Force Quit, Task Manager's End task) ends nothing:
//!
//! - On unix each tool runs in a process group of its own, which nothing ends when the process
//!   that started it dies: a `vagrant up` or a `terraform apply` ran to its end, holding the lab's
//!   Vagrant lock or Terraform state, with nobody reading its output (#301). Each tool's record
//!   names it, from just after it started until it ended.
//! - On Windows each tool is created inside a kill-on-close job object, which ends its tree when
//!   the process that holds the job dies, however it dies (#299). Its record is the backstop, for
//!   a tree the job didn't hold.
//!
//! A record says which process wrote it (its owner) and the tool, each by its id and
//! [`Identity`]. The next start ([`end_orphans`]) ends the tool each record of a dead owner names,
//! only when it is still that very process (an id now naming another process is left alone), and
//! removes the records.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use crate::platform::identity::{self, Identity};

/// The ledger's folder in the app's data folder.
pub const DIR: &str = "tools";

/// A record's extension.
const EXT: &str = "tool";

/// How long a streamed tool left behind has to stop once asked (SIGTERM, as the app's exit asks
/// it), before it is killed.
#[cfg(unix)]
const GRACE: std::time::Duration = std::time::Duration::from_secs(15);

/// The folder this process records its tools in, once set.
static FOLDER: OnceLock<PathBuf> = OnceLock::new();

/// Records this process's tools in `dir` from now on: the app's, and each deploy worker's, in the
/// app's data folder. The first folder given stays.
pub fn record_tools_in(dir: PathBuf) {
    if let Err(e) = std::fs::create_dir_all(&dir) {
        log::warn!("couldn't make the tools' ledger folder {}: {e} (a killed app's tools would run on)", dir.display());
    }
    let _ = FOLDER.set(dir);
}

/// A process as a record names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Proc {
    pid: u32,
    id: Identity,
}

impl Proc {
    /// Process `pid` as the OS describes it now.
    fn of(pid: u32) -> Option<Proc> {
        identity::of(pid).map(|id| Proc { pid, id })
    }

    /// Whether this very process still runs (not just one with its id).
    fn alive(self) -> bool {
        identity::of(self.pid) == Some(self.id)
    }

    /// A record's word for it: `<pid>:<identity>`.
    fn word(self) -> String {
        format!("{}:{}", self.pid, self.id)
    }

    fn parse(word: &str) -> Option<Proc> {
        let (pid, id) = word.split_once(':')?;
        Some(Proc { pid: pid.parse().ok()?, id: Identity::parse(id)? })
    }
}

/// This process, as its records name their owner. None when the OS doesn't describe it: then
/// nothing is recorded.
fn me() -> Option<Proc> {
    static ME: OnceLock<Option<Proc>> = OnceLock::new();
    *ME.get_or_init(|| Proc::of(std::process::id()))
}

/// What a record names: the tool its owner started; `stream`, one asked to stop before it is
/// killed (on unix), as the app's exit does.
#[derive(Debug, PartialEq, Eq)]
struct Record {
    owner: Proc,
    tool: Proc,
    stream: bool,
}

impl Record {
    /// The record's line: `tool <owner> <tool> stream|probe`.
    fn line(&self) -> String {
        format!("tool {} {} {}\n", self.owner.word(), self.tool.word(), if self.stream { "stream" } else { "probe" })
    }

    fn parse(text: &str) -> Option<Record> {
        let text = text.strip_suffix('\n')?.strip_prefix("tool ")?;
        let mut words = text.split(' ');
        let (owner, tool, how) = (Proc::parse(words.next()?)?, Proc::parse(words.next()?)?, words.next()?);
        let stream = match how {
            "stream" => true,
            "probe" => false,
            _ => return None,
        };
        words.next().is_none().then_some(Record { owner, tool, stream })
    }
}

/// A record written, removed when this drops.
#[derive(Debug)]
pub(super) struct Recorded(PathBuf);

impl Recorded {
    /// Records the tool `pid` just started (in a process group of its own, or its job); `stream`,
    /// a long run. None when no ledger is kept or the tool is gone already.
    pub(super) fn tool(pid: u32, stream: bool) -> Option<Recorded> {
        let (dir, owner) = (FOLDER.get()?, me()?);
        let tool = Proc::of(pid)?;
        write(dir, owner.pid, &Record { owner, tool, stream })
    }

    /// Leaves the record for the next start: a tool asked to stop as this process exits, which
    /// may outlive it.
    #[cfg(unix)]
    pub(super) fn keep(self) {
        std::mem::forget(self);
    }
}

impl Drop for Recorded {
    fn drop(&mut self) {
        if let Err(e) = crate::platform::fs::remove_if_present(&self.0) {
            log::warn!("couldn't remove the tool record {}: {e}", self.0.display());
        }
    }
}

/// Writes `record` in `dir` as a file of its own, named after its owner's id and a number this
/// process doesn't reuse. A record that can't be written is said once: the tools then run
/// unrecorded, as they did before the ledger.
fn write(dir: &Path, owner: u32, record: &Record) -> Option<Recorded> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    static SAID: AtomicBool = AtomicBool::new(false);
    let path = dir.join(format!("{owner}-{}.{EXT}", NEXT.fetch_add(1, Ordering::Relaxed)));
    match std::fs::write(&path, record.line()) {
        Ok(()) => Some(Recorded(path)),
        Err(e) => {
            if !SAID.swap(true, Ordering::Relaxed) {
                log::warn!("couldn't record a tool in {}: {e} (a killed app's tools would run on)", dir.display());
            }
            None
        }
    }
}

/// Ends what the records in `dir` of a process no longer running name, and removes them: the
/// app's start, for what a killed app or deploy worker left. The ids of the processes ended.
pub async fn end_orphans(dir: &Path) -> Vec<u32> {
    let Ok(read) = std::fs::read_dir(dir) else { return Vec::new() };
    let paths: Vec<PathBuf> = read.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == EXT)).collect();
    let mut ended = Vec::new();
    #[cfg(unix)]
    let mut stopping = Vec::new();
    for path in paths {
        let Some(record) = std::fs::read_to_string(&path).ok().and_then(|t| Record::parse(&t)) else {
            // Being written, or cut short as its owner died: removed once that owner is gone.
            if owner_in_name(&path).is_some_and(|pid| identity::of(pid).is_none()) {
                remove(&path);
            }
            continue;
        };
        if record.owner.alive() {
            continue;
        }
        let Record { tool, stream, .. } = record;
        if tool.alive() {
            log::warn!("ending tool {} a killed app or deploy worker left running", tool.pid);
            ended.push(tool.pid);
            #[cfg(unix)]
            if stream {
                signal(tool.pid, libc::SIGTERM);
                stopping.push(tool);
            } else {
                signal(tool.pid, libc::SIGKILL);
            }
            #[cfg(windows)]
            end_tree(tool.pid, stream);
        }
        remove(&path);
    }
    #[cfg(unix)]
    kill_once_stopped(stopping).await;
    ended
}

/// The owner's id in a record's file name (`<owner>-<n>.tool`).
fn owner_in_name(path: &Path) -> Option<u32> {
    path.file_stem()?.to_str()?.split_once('-')?.0.parse().ok()
}

fn remove(path: &Path) {
    if let Err(e) = crate::platform::fs::remove_if_present(path) {
        log::warn!("couldn't remove the tool record {}: {e}", path.display());
    }
}

/// Signals the process group `pid` leads.
#[cfg(unix)]
fn signal(pid: u32, signal: libc::c_int) {
    if let Err(e) = crate::platform::sys::signal_group(pid, signal) {
        log::warn!("couldn't end tool process group {pid}: {e}");
    }
}

/// Waits up to [`GRACE`] for the tools asked to stop, then kills the groups of those still running.
#[cfg(unix)]
async fn kill_once_stopped(mut stopping: Vec<Proc>) {
    let deadline = tokio::time::Instant::now() + GRACE;
    loop {
        stopping.retain(|tool| tool.alive());
        if stopping.is_empty() || tokio::time::Instant::now() >= deadline {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
    for tool in stopping {
        signal(tool.pid, libc::SIGKILL);
    }
}

/// Ends the tool `pid` and its tree (`taskkill /T /F`): Windows has no signal to ask it first,
/// so a streamed tool is ended as a probe is.
#[cfg(windows)]
fn end_tree(pid: u32, _stream: bool) {
    let mut taskkill = std::process::Command::new("taskkill");
    taskkill
        .args(["/T", "/F", "/PID", &pid.to_string()])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    if let Err(e) = super::headless_std(&mut taskkill).status() {
        log::warn!("couldn't run taskkill to end tool {pid}: {e}");
    }
}

#[cfg(test)]
#[path = "ledger_tests.rs"]
mod tests;
