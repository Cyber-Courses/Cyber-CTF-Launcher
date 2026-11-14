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
//! - On Windows a tool's job object ends its tree when the process that holds the job dies, but
//!   only once the tool is in it: a tool is created suspended and put in its job before it runs,
//!   and one created as the app died was in no job, suspended for good (#299). A record names the
//!   tool being created, from just before it is created until it is in its job.
//!
//! A record says which process wrote it (its owner: its id and [`Identity`]), and the tool, by its
//! [`Identity`] on unix, or by its program and when it was being created on Windows. The next start
//! ([`end_orphans`]) ends what each record of a dead owner names, only when it is still that very
//! process (an id now naming another process is left alone), and removes the records.

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

/// What a record names.
#[derive(Debug, PartialEq, Eq)]
enum Record {
    /// A tool running in its own process group (unix); `stream`: one asked to stop before it is
    /// killed, as the app's exit does.
    Tool { owner: Proc, tool: Proc, stream: bool },
    /// A tool being created (Windows): `program` (as started: `cmd` for a shimmed tool), from
    /// `since` on (as a process's creation time, in its [`Identity`]'s units).
    Spawn { owner: Proc, since: u64, program: String },
}

impl Record {
    fn owner(&self) -> Proc {
        match self {
            Record::Tool { owner, .. } | Record::Spawn { owner, .. } => *owner,
        }
    }

    /// The record's line: `tool <owner> <tool> stream|probe`, or `spawn <owner> <since> <program>`
    /// (the program last: a path may hold spaces).
    fn line(&self) -> String {
        match self {
            Record::Tool { owner, tool, stream } => format!("tool {} {} {}\n", owner.word(), tool.word(), if *stream { "stream" } else { "probe" }),
            Record::Spawn { owner, since, program } => format!("spawn {} {since} {program}\n", owner.word()),
        }
    }

    fn parse(text: &str) -> Option<Record> {
        let text = text.strip_suffix('\n')?;
        let (kind, rest) = text.split_once(' ')?;
        let (owner, rest) = rest.split_once(' ')?;
        let owner = Proc::parse(owner)?;
        match kind {
            "tool" => {
                let (tool, how) = rest.split_once(' ')?;
                let stream = match how {
                    "stream" => true,
                    "probe" => false,
                    _ => return None,
                };
                Some(Record::Tool { owner, tool: Proc::parse(tool)?, stream })
            }
            "spawn" => {
                let (since, program) = rest.split_once(' ')?;
                let program = (!program.is_empty()).then(|| program.to_owned())?;
                Some(Record::Spawn { owner, since: since.parse().ok()?, program })
            }
            _ => None,
        }
    }
}

/// A record written, removed when this drops.
#[derive(Debug)]
pub(super) struct Recorded(PathBuf);

impl Recorded {
    /// Records the tool `pid` just started in a process group of its own; `stream`, a long run.
    /// None when no ledger is kept or the tool is gone already.
    #[cfg(unix)]
    pub(super) fn tool(pid: u32, stream: bool) -> Option<Recorded> {
        let (dir, owner) = (FOLDER.get()?, me()?);
        let tool = Proc::of(pid)?;
        write(dir, owner.pid, &Record::Tool { owner, tool, stream })
    }

    /// Records the tool `program` about to be created (Windows: until it is in its job).
    #[cfg(windows)]
    pub(super) fn spawning(program: &std::ffi::OsStr) -> Option<Recorded> {
        let (dir, owner) = (FOLDER.get()?, me()?);
        let since = filetime(std::time::SystemTime::now())?;
        write(dir, owner.pid, &Record::Spawn { owner, since, program: program.to_string_lossy().into_owned() })
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
        if record.owner().alive() {
            continue;
        }
        match record {
            #[cfg(unix)]
            Record::Tool { tool, stream, .. } if tool.alive() => {
                log::warn!("ending tool process group {} a killed app or deploy worker left running", tool.pid);
                ended.push(tool.pid);
                if stream {
                    signal(tool.pid, libc::SIGTERM);
                    stopping.push(tool);
                } else {
                    signal(tool.pid, libc::SIGKILL);
                }
            }
            #[cfg(windows)]
            Record::Spawn { owner, since, program } => ended.extend(end_spawned(owner, since, &program)),
            _ => {}
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

/// Ends the tool a dead owner was creating as it died, if it is still there (suspended, in no
/// job): a process the owner started, of the program recorded, created since. The ids ended.
#[cfg(windows)]
fn end_spawned(owner: Proc, since: u64, program: &str) -> Vec<u32> {
    // The owner is gone: a process with its id now is a later one.
    let reused = identity::of(owner.pid).map(|id| id.start);
    let children: Vec<Child> =
        crate::platform::sys::child_processes(owner.pid).into_iter().filter_map(|(pid, image)| Some(Child { pid, image, id: identity::of(pid)? })).collect();
    let orphans = window_orphans(owner, since, program, reused, &children);
    for pid in &orphans {
        log::warn!("ending tool process {pid} ({program}), left suspended outside its job by a killed app or deploy worker");
        crate::platform::sys::terminate(*pid);
    }
    orphans
}

/// A process another started, as Windows lists it.
#[cfg(any(windows, test))]
struct Child {
    pid: u32,
    /// Its image's file name (`docker.exe`).
    image: String,
    id: Identity,
}

/// How much earlier than the moment recorded before it was created a process's creation time
/// may read (Windows stores it from a coarser clock): a second, in 100 ns ticks.
#[cfg(any(windows, test))]
const CREATION_SLACK: u64 = 10_000_000;

/// Which of `children` (the processes `owner`'s id started) is the tool `program` it was creating
/// from `since` on: of `owner`'s boot, created since (and before `reused`, when a later process
/// has the owner's id now: what it started is its own), of the program recorded.
#[cfg(any(windows, test))]
fn window_orphans(owner: Proc, since: u64, program: &str, reused: Option<u64>, children: &[Child]) -> Vec<u32> {
    children
        .iter()
        .filter(|c| c.id.boot == owner.id.boot && c.id.start.saturating_add(CREATION_SLACK) >= since && c.id.start >= owner.id.start)
        .filter(|c| reused.is_none_or(|r| c.id.start < r))
        .filter(|c| program_name(&c.image).eq_ignore_ascii_case(program_name(program)))
        .map(|c| c.pid)
        .collect()
}

/// A program's name as an image is named: its file name, without `.exe`.
#[cfg(any(windows, test))]
fn program_name(program: &str) -> &str {
    let name = program.rsplit(['/', '\\']).next().unwrap_or(program);
    match name.len().checked_sub(4).and_then(|at| name.get(at..).map(|ext| (at, ext))) {
        Some((at, ext)) if ext.eq_ignore_ascii_case(".exe") => name.get(..at).unwrap_or(name),
        _ => name,
    }
}

/// `t` as Windows stores a process's creation time: 100 ns ticks since 1601.
#[cfg(any(windows, test))]
fn filetime(t: std::time::SystemTime) -> Option<u64> {
    const UNIX_EPOCH_IN_TICKS: u64 = 116_444_736_000_000_000;
    let since_epoch = t.duration_since(std::time::UNIX_EPOCH).ok()?;
    u64::try_from(since_epoch.as_nanos() / 100).ok()?.checked_add(UNIX_EPOCH_IN_TICKS)
}

#[cfg(test)]
#[path = "ledger_tests.rs"]
mod tests;
