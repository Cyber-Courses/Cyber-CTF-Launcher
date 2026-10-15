//! Worker processes: starting one detached, telling a live worker from a stale pidfile, listing
//! the running ones, and stopping one with everything it started.

use std::path::Path;
use std::time::Duration;

use crate::AppHandle;

use super::Job;
use super::files::{self, Files, read_worker};
use crate::error::{Error, Result};

/// A worker process as its pidfile records it: its id and, when it could be read, its start
/// time (seconds since the epoch, as the OS reports it). The start time tells the worker from
/// an unrelated process that got its id after it was gone (Windows recycles ids quickly).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Worker {
    pub pid: u32,
    pub started: Option<u64>,
}

impl Worker {
    /// A pidfile's text: `<pid> <start time>`, or a bare `<pid>` (older versions).
    pub fn parse(text: &str) -> Option<Worker> {
        let mut words = text.split_whitespace();
        let pid = words.next()?.parse().ok()?;
        Some(Worker { pid, started: words.next().and_then(|s| s.parse().ok()) })
    }

    /// The pidfile's text for this worker.
    fn line(&self) -> String {
        match self.started {
            Some(t) => format!("{} {t}", self.pid),
            None => self.pid.to_string(),
        }
    }
}

/// A worker just started: its files and its process.
pub struct Spawned {
    pub files: Files,
    pub worker: Worker,
}

/// Takes the pidfile for a new worker: created only if no other worker holds it (a stale one,
/// naming no live worker, is replaced). Refuses while a worker of the same key runs, so two
/// starts racing each other never spawn two workers over the same lab.
pub fn claim(f: &Files) -> Result<std::fs::File> {
    let create = || std::fs::OpenOptions::new().write(true).create_new(true).open(&f.pid);
    match create() {
        Ok(file) => Ok(file),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            if f.worker().is_some_and(|w| alive(&w)) {
                return Err(Error::coded(crate::error::Code::AlreadyInProgress));
            }
            let _ = std::fs::remove_file(&f.pid);
            create().map_err(Error::Io)
        }
        Err(e) => Err(Error::Io(e)),
    }
}

/// Starts the worker for `job`, detached from this process, under the pidfile `pidfile` the
/// caller claimed (see [`claim`]). `Err` when it can't be started (the pidfile is then given
/// back); the caller then deploys in-process as a fallback.
pub fn spawn(app: &AppHandle, job: &Job, pidfile: std::fs::File) -> Result<Spawned> {
    let f = files::of(app, &job.key())?;
    let path = f.pid.clone();
    let job_file = f.job.clone();
    let started = start(f, job, pidfile);
    if started.is_err() {
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(&job_file);
    }
    started
}

fn start(f: Files, job: &Job, mut pidfile: std::fs::File) -> Result<Spawned> {
    use std::io::Write as _;
    crate::runtime::validate_id(&job.lab_id)?;
    // A fresh run: no verdict or log left from an earlier one.
    let _ = std::fs::remove_file(&f.status);
    if job.op == super::Op::Launch {
        let _ = std::fs::remove_file(&f.launch_status);
    }
    let log = write_job(&f, job)?;
    let exe = std::env::current_exe().map_err(Error::Io)?;
    let mut cmd = std::process::Command::new(exe);
    cmd.arg("deploy").arg("--job").arg(&f.job).stdin(std::process::Stdio::null()).stdout(log.try_clone().map_err(Error::Io)?).stderr(log);
    detach(&mut cmd, true);
    // A UAC prompt it raises (Vagrant on Hyper-V) comes up for the app's window.
    #[cfg(windows)]
    if let Some(hwnd) = crate::exec::owner_window() {
        cmd.env(crate::exec::OWNER_WINDOW_ENV, hwnd.to_string());
    }
    let mut child = match cmd.spawn() {
        // Windows: the job the app runs in forbids breaking away; start the worker inside it.
        Err(e) if cfg!(windows) && e.raw_os_error() == Some(ERROR_ACCESS_DENIED) => {
            log::warn!("deploy worker can't leave the app's job object; it ends if that job is closed");
            detach(&mut cmd, false);
            cmd.spawn().map_err(Error::Io)?
        }
        spawned => spawned.map_err(Error::Io)?,
    };
    let pid = child.id();
    let worker = Worker { pid, started: observe(pid).map(|o| o.started) };
    pidfile.write_all(worker.line().as_bytes()).map_err(Error::Io)?;
    drop(pidfile);
    // Reaped when it exits: unreaped, a finished worker stays a zombie for as long as the app
    // runs, and a zombie still answers `kill -0`. Its pidfile goes with it (only while it still
    // names this worker): a worker that finished before the line above wrote the pidfile would
    // otherwise leave one behind.
    let path = f.pid.clone();
    std::thread::spawn(move || {
        let _ = child.wait();
        remove_pidfile_of(&path, pid);
    });
    Ok(Spawned { files: f, worker })
}

/// Writes the job for its worker and opens its fresh log, both owner-only: the job carries the
/// launch token (the worker removes it when the job ends), the log what the tools printed.
fn write_job(f: &Files, job: &Job) -> Result<std::fs::File> {
    crate::private_fs::write(&f.job, serde_json::to_vec(job).map_err(|e| Error::Invalid(e.to_string()))?).map_err(Error::Io)?;
    crate::private_fs::create(&f.log).map_err(Error::Io)
}

/// Removes the pidfile at `path` only while it still names the worker `pid`: a newer worker's
/// pidfile is left alone.
pub fn remove_pidfile_of(path: &Path, pid: u32) {
    if read_worker(path).is_some_and(|w| w.pid == pid) {
        let _ = std::fs::remove_file(path);
    }
}

/// What CreateProcess answers when the app's job object doesn't allow breaking away.
const ERROR_ACCESS_DENIED: i32 = 5;

/// Its own process group (and session leader on unix): closing the app's terminal or the app
/// itself sends it nothing, and stopping it later stops its children too.
///
/// On Windows, also out of the app's job object when `breakaway`: an app started by a launcher,
/// a terminal or an installer can sit in a job that kills its whole tree when it closes, which
/// would cut the deploy short.
#[cfg_attr(not(windows), allow(unused_variables))]
fn detach(cmd: &mut std::process::Command, breakaway: bool) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
        let flags = DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP;
        cmd.creation_flags(if breakaway { flags | CREATE_BREAKAWAY_FROM_JOB } else { flags });
    }
}

/// A process as the OS describes it right now.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Observed {
    /// When it started (seconds since the epoch).
    pub started: u64,
    /// Exited but not reaped yet (or being torn down).
    pub zombie: bool,
    /// Its command line, arguments joined by spaces (empty when the OS wouldn't say).
    pub cmdline: String,
    /// Its executable's file name, when known.
    pub exe: Option<String>,
}

/// What the OS says about process `pid`, if it exists.
///
/// On macOS sysinfo misreads a live process now and then, more often while other processes
/// start: it first lists every process id and finds nothing at all when processes were started
/// between its two `proc_listallpids` calls, and it reads the arguments with two
/// `KERN_PROCARGS2` sysctls, the second of which can fail, leaving the command line empty. A
/// live worker then read as gone (or as another program), and its pidfile was swept as stale.
/// So a process that the OS still has is looked at again, a few times, until sysinfo sees it
/// (on macOS, with its command line).
pub(super) fn observe(pid: u32) -> Option<Observed> {
    let mut last = None;
    for _ in 0..5 {
        match observe_once(pid) {
            // Elsewhere an empty command line is what the OS says (Windows: an elevated process).
            Some(seen) if seen.zombie || !seen.cmdline.is_empty() || !cfg!(target_os = "macos") => return Some(seen),
            Some(seen) => last = Some(seen),
            None if last.is_none() && !exists(pid) => return None,
            None => {}
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    last
}

/// Whether process `pid` exists, by signal 0 (nothing is sent): it does when the signal could
/// be sent, or was refused (another user's process).
#[cfg(unix)]
fn exists(pid: u32) -> bool {
    // A pid past i32 would name a process group once negative.
    let Ok(pid) = libc::pid_t::try_from(pid) else { return false };
    if pid <= 0 {
        return false;
    }
    // SAFETY: kill with signal 0 only checks the process exists.
    let sent = unsafe { libc::kill(pid, 0) } == 0;
    sent || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

/// Elsewhere sysinfo reads one process at a time: what it misses is gone.
#[cfg(not(unix))]
fn exists(_pid: u32) -> bool {
    false
}

/// [`observe`], one look.
fn observe_once(pid: u32) -> Option<Observed> {
    use sysinfo::{Pid, ProcessRefreshKind, ProcessStatus, ProcessesToUpdate, System, UpdateKind};
    let pid = Pid::from_u32(pid);
    let mut sys = System::new();
    sys.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[pid]),
        true,
        ProcessRefreshKind::nothing().with_cmd(UpdateKind::Always).with_exe(UpdateKind::Always),
    );
    let p = sys.process(pid)?;
    Some(Observed {
        started: p.start_time(),
        zombie: matches!(p.status(), ProcessStatus::Zombie | ProcessStatus::Dead),
        cmdline: p.cmd().iter().map(|a| a.to_string_lossy()).collect::<Vec<_>>().join(" "),
        exe: p.exe().and_then(Path::file_name).map(|n| n.to_string_lossy().into_owned()),
    })
}

/// Start times read twice can differ by the rounding of the boot time: within this many
/// seconds they are the same process.
const START_SLACK: u64 = 2;

/// Whether `worker` is a live deploy worker: running (not a zombie), this app's `deploy --job`
/// process, and (when its start time was recorded) the very process that was started, not
/// another one that got the id of a worker long gone.
pub fn alive(worker: &Worker) -> bool {
    let this_exe = std::env::current_exe().ok().and_then(|e| e.file_name().map(|n| n.to_string_lossy().into_owned()));
    is_live_worker(worker, observe(worker.pid).as_ref(), this_exe.as_deref())
}

/// A pidfile line for `pid` as the app writes it (with the process's start time), for tests
/// that stand a process in for a worker.
#[cfg(all(test, unix))]
pub(crate) fn pidfile_line(pid: u32) -> String {
    Worker { pid, started: observe(pid).map(|o| o.started) }.line()
}

/// [`alive`] from what the OS said about the process (`None`: no such process) and this app's
/// executable name.
fn is_live_worker(worker: &Worker, seen: Option<&Observed>, this_exe: Option<&str>) -> bool {
    let Some(seen) = seen else { return false };
    if seen.zombie {
        return false;
    }
    if worker.started.is_some_and(|t| seen.started.abs_diff(t) > START_SLACK) {
        return false;
    }
    // A pidfile without a start time (older versions) can only be matched by what the process
    // looks like: another program that got the id, whatever its arguments say, is not a worker.
    if worker.started.is_none()
        && let (Some(exe), Some(this)) = (seen.exe.as_deref(), this_exe)
        && exe != this
    {
        return false;
    }
    if seen.cmdline.is_empty() {
        // The OS wouldn't give its arguments: at least the same program as this app.
        return seen.exe.is_some() && seen.exe.as_deref() == this_exe;
    }
    is_worker(&seen.cmdline)
}

/// A deploy worker's command line: `<app> deploy --job <file>`.
fn is_worker(cmdline: &str) -> bool {
    cmdline.contains(" deploy ") && cmdline.contains("--job")
}

/// The keys (lab ids, and `<lab>.attack` for attack VM starts) whose worker is running right
/// now: a pidfile naming a live process. A pidfile whose process is gone (the worker finished,
/// or died) is removed on the way.
pub fn running(app: &AppHandle) -> Vec<String> {
    let Ok(d) = files::dir(app) else { return Vec::new() };
    running_in(&d)
}

/// [`running`] for the workers' folder `d`.
fn running_in(d: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(d) else { return Vec::new() };
    let mut out = Vec::new();
    for e in entries.flatten() {
        let p = e.path();
        let Some(key) = pidfile_key(&p) else { continue };
        match read_worker(&p) {
            Some(w) if alive(&w) => out.push(key),
            // Just claimed, its worker not started yet (see `claim`): not stale.
            None if std::fs::read(&p).is_ok_and(|b| b.is_empty()) => {}
            _ => {
                let _ = std::fs::remove_file(&p);
            }
        }
    }
    out.sort();
    out
}

/// The worker key a pidfile (`<key>.pid`) is for.
fn pidfile_key(path: &Path) -> Option<String> {
    if path.extension().and_then(|x| x.to_str()) != Some("pid") {
        return None;
    }
    path.file_stem().and_then(|s| s.to_str()).map(str::to_string)
}

/// The jobs whose worker runs right now, each with the step its log is at (for the sidebar).
pub fn running_jobs(app: &AppHandle) -> Vec<(Job, Option<String>)> {
    let Ok(d) = files::dir(app) else { return Vec::new() };
    jobs_in(&d)
}

/// The operation a worker key stands for when its job file can't be read: a launch of that
/// lab, or the attack VM beside it (`<lab>.attack`).
fn job_for_key(key: &str) -> Job {
    match key.strip_suffix(".attack") {
        Some(lab) => Job::on_lab(lab, super::Op::AttackVm { box_name: String::new() }),
        None => Job::on_lab(key, super::Op::Launch),
    }
}

/// [`running_jobs`] for the workers' folder `d`.
fn jobs_in(d: &Path) -> Vec<(Job, Option<String>)> {
    running_in(d)
        .into_iter()
        .map(|key| {
            let f = Files::of(d, &key);
            // A job file cut short (a full disk, a crash while it was written) must not hide a
            // worker that is alive: it is listed as the plain operation its key names.
            let job: Job = std::fs::read(&f.job).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_else(|| job_for_key(&key));
            let step = super::progress::step_of(&f.log);
            (job, step)
        })
        .collect()
}

/// Stops the worker of `key` (a lab id, or its attack VM's key), with everything it started
/// (its process group), if one runs.
pub fn kill(app: &AppHandle, key: &str) {
    let Ok(f) = files::of(app, key) else { return };
    kill_files(&f);
}

/// [`kill`] for a worker's files.
fn kill_files(f: &Files) {
    let Some(worker) = f.worker() else { return };
    if alive(&worker) {
        #[cfg(unix)]
        signal_group(worker.pid, libc::SIGTERM);
        #[cfg(windows)]
        {
            let _ = crate::exec::headless_std(std::process::Command::new("taskkill").args(["/PID", &worker.pid.to_string(), "/T", "/F"])).status();
        }
    }
    remove_pidfile_of(&f.pid, worker.pid);
    // A worker killed outside its own end (Windows, or SIGKILL) leaves its job behind.
    let _ = std::fs::remove_file(&f.job);
}

/// Sends `signal` to the process group `pid` leads: the worker and its vagrant/docker/terraform.
/// A syscall, not `kill -TERM -<pid>`: procps' `kill` (Debian, Ubuntu) reads `-<pid>` as an
/// option and signals the group named by its first digit, so `-1234` sent SIGTERM to every
/// process of the user (`kill(-1)`) instead of the worker's group.
#[cfg(unix)]
fn signal_group(pid: u32, signal: libc::c_int) {
    let Ok(group) = libc::pid_t::try_from(pid) else { return };
    if group <= 1 {
        return;
    }
    // SAFETY: plain syscall on a process group the worker leads (`process_group(0)` at spawn).
    unsafe {
        libc::killpg(group, signal);
    }
}

/// Half-second checks a Terraform run gets to stop on its own once interrupted: it finishes the
/// resources it is creating and records them in its state. Ten minutes: an Azure or GCP VM
/// alone can take several to come up, and a second signal makes Terraform exit at once, which
/// is the orphaned-resource case this wait is here to avoid.
const TERRAFORM_GRACE_CHECKS: u32 = 1200;

/// Half-second checks a Terraform apply gets to end on its own on Windows when no Ctrl+C reached
/// it (twenty minutes, a whole lab's worth).
#[cfg(windows)]
const TERRAFORM_APPLY_WAIT_CHECKS: u32 = 2400;

/// Whether `job` is a launch on a Terraform-driven host (a cloud account, Proxmox), given that
/// host's provider: a worker mid-`terraform apply` must not be killed outright.
#[cfg(any(windows, test))]
fn terraform_launch(job: &Job, host_provider: Option<crate::runtime::providers::Provider>) -> bool {
    job.op == super::Op::Launch && host_provider.is_some_and(|p| crate::runtime::server::terraform_target(p).is_some())
}

/// [`terraform_launch`] for the job a worker's files describe.
#[cfg(windows)]
fn launches_terraform(app: &AppHandle, f: &Files) -> bool {
    let Some(job) = std::fs::read(&f.job).ok().and_then(|b| serde_json::from_slice::<Job>(&b).ok()) else { return false };
    let provider = job.host.as_deref().and_then(|h| crate::runtime::server::host_provider(app, h));
    terraform_launch(&job, provider)
}

/// [`kill`], then waits until the worker is gone (SIGKILL to its group after 20 s), so what
/// follows (a teardown) never races a `compose up` or `vagrant up` that is still creating what
/// it removes. A worker running Terraform is interrupted first instead: killed mid-apply,
/// Terraform never records what it was creating, and the destroy that follows leaves those
/// resources running (and billing). Interrupted, it stops cleanly with its state saved, the
/// worker ends with it, and the destroy that follows sees everything.
pub async fn kill_and_wait(app: &AppHandle, key: &str) {
    let Ok(f) = files::of(app, key) else { return };
    let Some(worker) = claimed_worker(&f).await.filter(alive) else { return };
    if interrupt_terraform(worker.pid) && gone_within(&worker, TERRAFORM_GRACE_CHECKS).await {
        remove_pidfile_of(&f.pid, worker.pid);
        return;
    }
    // Windows, when no Ctrl+C reached its Terraform (none found yet, or its console refused it):
    // `taskkill` mid-apply leaves what it was creating out of its state (the destroy that follows
    // then leaves those resources running, and billing). The apply is waited for instead, bounded,
    // so the destroy sees everything.
    #[cfg(windows)]
    if launches_terraform(app, &f) {
        log::info!("stop of {key}: waiting for its Terraform apply to end before tearing down");
        if gone_within(&worker, TERRAFORM_APPLY_WAIT_CHECKS).await {
            remove_pidfile_of(&f.pid, worker.pid);
            return;
        }
    }
    kill(app, key);
    if gone_within(&worker, 40).await {
        return;
    }
    #[cfg(unix)]
    signal_group(worker.pid, libc::SIGKILL);
    gone_within(&worker, 10).await;
}

/// Looks (quarter-second apart) a stop gives a just-claimed pidfile to name its worker.
const CLAIM_CHECKS: u32 = 20;

/// The worker the pidfile names, waiting while it is claimed but still empty (`claim` creates
/// it before the worker is spawned): a stop in that window found nothing to stop, and the worker
/// then took the lab and ran its job after the stop.
async fn claimed_worker(f: &Files) -> Option<Worker> {
    for _ in 0..CLAIM_CHECKS {
        if !std::fs::read(&f.pid).is_ok_and(|b| b.is_empty()) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    f.worker()
}

/// Sends SIGINT to the Terraform runs in process group `group` (the host binary, or the `docker
/// run` of its container, which hands the signal on): Terraform's own clean stop. Whether one
/// was found.
#[cfg(unix)]
fn interrupt_terraform(group: u32) -> bool {
    let pgrep = |args: &[&str]| std::process::Command::new("pgrep").args(args).output().map(|o| pids(&String::from_utf8_lossy(&o.stdout))).unwrap_or_default();
    // The worker's group, and those of the tools it streams: each leads its own (exec::stream).
    let mut groups = vec![group];
    groups.extend(pgrep(&["-P", &group.to_string()]));
    let groups = groups.iter().map(u32::to_string).collect::<Vec<_>>().join(",");
    let mut found = pgrep(&["-g", &groups, "-x", "terraform"]);
    found.extend(pgrep(&["-g", &groups, "-f", "hashicorp/terraform"]));
    for pid in &found {
        let _ = std::process::Command::new("kill").args(["-INT", &pid.to_string()]).status();
    }
    !found.is_empty()
}

/// Windows: a Ctrl+C on the console of each Terraform run under the worker `root` (the host
/// binary, or the `docker run` of its container, which hands it on as SIGINT). Whether one was
/// interrupted.
#[cfg(windows)]
fn interrupt_terraform(root: u32) -> bool {
    let found = terraform_runs_under(root);
    found.into_iter().filter(|&pid| crate::platform::console::interrupt(pid)).count() > 0
}

/// The Terraform runs among `root`'s descendants, from a snapshot of all processes.
#[cfg(windows)]
fn terraform_runs_under(root: u32) -> Vec<u32> {
    use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
    let mut sys = System::new();
    sys.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::nothing().with_cmd(UpdateKind::Always));
    let procs: Vec<Proc> = sys
        .processes()
        .iter()
        .map(|(pid, p)| Proc {
            pid: pid.as_u32(),
            parent: p.parent().map(|p| p.as_u32()),
            name: p.name().to_string_lossy().to_lowercase(),
            cmdline: p.cmd().iter().map(|a| a.to_string_lossy()).collect::<Vec<_>>().join(" "),
        })
        .collect();
    terraform_pids(&procs, root)
}

#[cfg_attr(not(windows), allow(dead_code))]
struct Proc {
    pid: u32,
    parent: Option<u32>,
    name: String,
    cmdline: String,
}

/// The Terraform runs below `root` in `procs`: `terraform.exe`, or a `docker run` of the
/// hashicorp/terraform image. Not their own children (providers share their console, so the one
/// Ctrl+C reaches them too).
#[cfg_attr(not(windows), allow(dead_code))]
fn terraform_pids(procs: &[Proc], root: u32) -> Vec<u32> {
    let is_terraform =
        |p: &Proc| p.name == "terraform.exe" || p.name == "terraform" || (p.name.starts_with("docker") && p.cmdline.contains("hashicorp/terraform"));
    let parent_of = |pid: u32| procs.iter().find(|p| p.pid == pid).and_then(|p| p.parent);
    // Under `root`, and not under another Terraform run. Depth-capped: a pid can be reused, so
    // the parent chain read from a snapshot may loop.
    let under_root = |p: &Proc| {
        let mut cur = p.parent;
        for _ in 0..32 {
            match cur {
                Some(pid) if pid == root => return true,
                Some(pid) if procs.iter().any(|q| q.pid == pid && is_terraform(q)) => return false,
                Some(pid) => cur = parent_of(pid),
                None => return false,
            }
        }
        false
    };
    procs.iter().filter(|p| p.pid != root && is_terraform(p) && under_root(p)).map(|p| p.pid).collect()
}

/// The process ids `pgrep` printed, one per line.
#[cfg(any(unix, test))]
fn pids(out: &str) -> Vec<u32> {
    out.lines().filter_map(|l| l.trim().parse().ok()).collect()
}

/// Whether `worker` stops being a live worker within `checks` half-second checks.
async fn gone_within(worker: &Worker, checks: u32) -> bool {
    for _ in 0..checks {
        if !alive(worker) {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Only a launch on a host Terraform drives is waited for: a VM lab on ESXi (Vagrant) or a
    /// park can be ended as before.
    #[test]
    fn a_launch_on_a_terraform_host_is_told_apart() {
        use crate::runtime::providers::Provider;
        let launch = Job { host: Some("h".into()), ..Job::on_lab("lab-1", super::super::Op::Launch) };
        assert!(terraform_launch(&launch, Some(Provider::Aws)));
        assert!(terraform_launch(&launch, Some(Provider::Proxmox)));
        assert!(!terraform_launch(&launch, Some(Provider::VmwareEsxi)));
        assert!(!terraform_launch(&launch, None));
        let park = Job {
            host: Some("h".into()),
            ..Job::on_lab("lab-1", super::super::Op::Park { runtime: crate::runtime::Runtime::Vm, mode: crate::runtime::Park::Pause })
        };
        assert!(!terraform_launch(&park, Some(Provider::Aws)));
    }

    #[test]
    fn the_terraform_runs_found_are_the_workers_and_not_their_providers() {
        let p = |pid, parent, name: &str, cmdline: &str| Proc { pid, parent: Some(parent), name: name.into(), cmdline: cmdline.into() };
        let procs = [
            p(100, 1, "cyberctf-desktop.exe", "cyberctf-desktop.exe deploy --job a.json"),
            p(110, 100, "terraform.exe", "terraform apply -auto-approve"),
            p(111, 110, "terraform-provider-aws_v5.exe", ""),
            // Nested terraform (a module calling out): its parent run's Ctrl+C reaches it.
            p(112, 110, "terraform.exe", "terraform version"),
            p(120, 100, "docker.exe", "docker run --rm hashicorp/terraform:1.9.8 apply"),
            p(130, 100, "docker.exe", "docker compose up"),
            // Another worker's run, and a pid cycle from reuse: neither is this worker's.
            p(200, 1, "cyberctf-desktop.exe", "cyberctf-desktop.exe deploy --job b.json"),
            p(210, 200, "terraform.exe", "terraform apply"),
            p(300, 301, "terraform.exe", ""),
            p(301, 300, "cmd.exe", ""),
        ];
        let mut found = terraform_pids(&procs, 100);
        found.sort();
        assert_eq!(found, [110, 120]);
        assert!(terraform_pids(&procs, 999).is_empty());
    }

    #[test]
    fn only_a_deploy_worker_counts_as_one() {
        assert!(is_worker("/usr/bin/cyberctf-desktop deploy --job /data/deploys/lab-1.json"));
        assert!(!is_worker("/usr/bin/firefox-esr --new-window"));
        assert!(!is_worker(""));
    }

    fn seen(started: u64, cmdline: &str) -> Observed {
        Observed { started, zombie: false, cmdline: cmdline.into(), exe: Some("cyberctf-desktop".into()) }
    }

    const WORKER: &str = "/usr/bin/cyberctf-desktop deploy --job /data/deploys/lab-1.json";

    #[test]
    fn a_recycled_pid_is_not_the_worker() {
        // Regression: on Windows a pidfile left by a worker long gone named whatever process got
        // its id next, and Stop taskkilled that unrelated process (and its tree). The start time
        // recorded with the pid tells them apart.
        let w = Worker { pid: 4242, started: Some(1_760_000_000) };
        assert!(is_live_worker(&w, Some(&seen(1_760_000_000, WORKER)), None));
        assert!(is_live_worker(&w, Some(&seen(1_760_000_001, WORKER)), None), "rounding");
        assert!(!is_live_worker(&w, Some(&seen(1_760_090_000, WORKER)), None), "another process with the same id");
        assert!(!is_live_worker(&w, Some(&seen(1_760_000_000, "C:\\Windows\\notepad.exe")), None), "not a worker");
        assert!(!is_live_worker(&w, None, None), "no such process");
        let zombie = Observed { zombie: true, ..seen(1_760_000_000, WORKER) };
        assert!(!is_live_worker(&w, Some(&zombie), None));
        // A pidfile from an older version (no start time) still needs a worker's command line.
        let legacy = Worker { pid: 4242, started: None };
        assert!(is_live_worker(&legacy, Some(&seen(1, WORKER)), None));
        assert!(!is_live_worker(&legacy, Some(&seen(1, "/usr/bin/firefox")), None));
        // Arguments the OS won't give: the same program as this app, or nothing.
        assert!(is_live_worker(&w, Some(&seen(1_760_000_000, "")), Some("cyberctf-desktop")));
        assert!(!is_live_worker(&w, Some(&seen(1_760_000_000, "")), Some("other")));
        assert!(!is_live_worker(&w, Some(&Observed { exe: None, ..seen(1_760_000_000, "") }), None));
    }

    #[test]
    fn pgrep_output_reads_as_pids() {
        assert_eq!(pids("123\n 456\n\nnope\n"), [123, 456]);
        assert!(pids("").is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn a_stop_interrupts_terraform_rather_than_killing_it() {
        // Regression: Stop during a cloud `terraform apply` SIGTERMed the worker's whole group,
        // then SIGKILLed it, so Terraform never recorded what it was creating and the destroy
        // that followed left those resources running. Terraform alone gets SIGINT now (its clean
        // stop). Terraform here is a stand-in for its container run (`… hashicorp/terraform …`),
        // in a group of its own like a worker's.
        use std::os::unix::process::CommandExt;
        let mut run =
            std::process::Command::new("sh").args(["-c", "sleep 30 & wait", "docker", "run", "hashicorp/terraform:1.9.8"]).process_group(0).spawn().unwrap();
        let group = run.id();
        // A group with no Terraform in it: nothing is sent.
        let mut other = std::process::Command::new("sleep").arg("30").process_group(0).spawn().unwrap();
        assert!(!interrupt_terraform(other.id()));
        // `pgrep` may need a moment to see the new process by name.
        let found = (0..20).any(|_| {
            std::thread::sleep(Duration::from_millis(50));
            interrupt_terraform(group)
        });
        assert!(found, "terraform found in its group");
        let status = run.wait().unwrap();
        // Ended by the interrupt (its sleep alone would take 30 s), however the shell reports it.
        assert!(!status.success(), "{status:?}");
        other.kill().unwrap();
        other.wait().unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn a_stop_reaches_terraform_in_a_group_of_its_own() {
        // A worker streams each tool in a process group of its own (so a cancel ends the tool's
        // whole tree), out of the worker's group: Terraform's interrupt still finds it there.
        // The stand-in worker starts it in a group of its own (perl's setpgrp, with SIGINT back
        // to its default: `&` ignores it); the image name comes from the environment, so only
        // the stand-in Terraform's command line names it.
        use std::os::unix::process::CommandExt;
        let run = r#"perl -e '$SIG{INT} = "DEFAULT"; setpgrp(0, 0); exec @ARGV' sh -c 'sleep 30 & wait' docker run "$IMAGE" & wait"#;
        let mut worker = std::process::Command::new("sh").args(["-c", run]).env("IMAGE", "hashicorp/terraform:1.9.8").process_group(0).spawn().unwrap();
        let found = (0..40).any(|_| {
            std::thread::sleep(Duration::from_millis(50));
            interrupt_terraform(worker.id())
        });
        assert!(found, "terraform found in its own group under the worker");
        let started = std::time::Instant::now();
        let waiter = std::thread::spawn(move || worker.wait());
        while !waiter.is_finished() && started.elapsed() < Duration::from_secs(10) {
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(waiter.is_finished(), "the interrupted run ended, and the worker with it");
    }

    #[test]
    fn pidfile_lines_read_back() {
        for w in [Worker { pid: 7, started: Some(1_760_000_000) }, Worker { pid: 7, started: None }] {
            assert_eq!(Worker::parse(&w.line()), Some(w));
        }
        assert_eq!(Worker::parse("  "), None);
    }

    #[test]
    fn only_pidfiles_name_a_worker() {
        assert_eq!(pidfile_key(Path::new("/d/lab-1.pid")).as_deref(), Some("lab-1"));
        assert_eq!(pidfile_key(Path::new("/d/lab-1.attack.pid")).as_deref(), Some("lab-1.attack"));
        assert_eq!(pidfile_key(Path::new("/d/lab-1.log")), None);
    }

    #[test]
    fn a_finished_or_foreign_process_is_not_alive() {
        // This test binary is alive but isn't a worker; a reaped child is gone.
        let me = Worker { pid: std::process::id(), started: observe(std::process::id()).map(|o| o.started) };
        assert!(me.started.is_some(), "the OS describes this very process");
        assert!(!alive(&me));
        let mut child =
            if cfg!(windows) { std::process::Command::new("cmd").args(["/C", "exit"]).spawn() } else { std::process::Command::new("true").spawn() }.unwrap();
        let pid = child.id();
        child.wait().unwrap();
        assert!(!alive(&Worker { pid, started: None }));
    }

    #[cfg(unix)]
    #[test]
    fn a_process_exists_until_it_is_reaped() {
        assert!(super::exists(std::process::id()));
        let mut child = std::process::Command::new("true").spawn().unwrap();
        let pid = child.id();
        child.wait().unwrap();
        assert!(!super::exists(pid));
        assert!(observe(pid).is_none());
        // Never a process group: a pid past i32, or 0.
        assert!(!super::exists(4_000_000_000));
        assert!(!super::exists(0));
    }

    /// A process whose command line reads like a worker's (`… deploy --job …`), sleeping.
    #[cfg(unix)]
    fn fake_worker() -> std::process::Child {
        exec_done(std::process::Command::new("sh").args(["-c", "sleep 20; :", "deploy", "--job", "x"]).spawn().unwrap())
    }

    /// `child` once the OS shows it as the program it runs: right after `spawn` it can still be
    /// the forked test binary, whose command line is no worker's (the tests were flaky).
    #[cfg(unix)]
    fn exec_done(child: std::process::Child) -> std::process::Child {
        for _ in 0..200 {
            if observe(child.id()).is_some_and(|o| is_worker(&o.cmdline)) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        child
    }

    #[cfg(unix)]
    #[test]
    fn a_live_worker_is_alive_only_with_its_own_start_time() {
        let mut child = fake_worker();
        let pid = child.id();
        let started = observe(pid).unwrap().started;
        assert!(alive(&Worker { pid, started: Some(started) }));
        assert!(!alive(&Worker { pid, started: Some(started - 3600) }));
        child.kill().unwrap();
        child.wait().unwrap();
        assert!(!alive(&Worker { pid, started: Some(started) }));
    }

    #[test]
    fn a_pidfile_is_claimed_once_and_a_stale_one_replaced() {
        let dir = crate::deploy_worker::files::tests::temp_dir("claim");
        let f = Files::of(&dir, "lab-1");
        drop(claim(&f).unwrap());
        assert!(f.pid.exists());
        // A worker long gone: its pidfile is stale and is taken over.
        std::fs::write(&f.pid, "4000000000 1").unwrap();
        drop(claim(&f).unwrap());
        assert_eq!(std::fs::read_to_string(&f.pid).unwrap(), "");
        std::fs::remove_dir_all(dir).ok();
    }

    #[cfg(unix)]
    #[test]
    fn a_second_start_is_refused_while_the_first_worker_runs() {
        // Regression: two launches racing each other both saw no worker and spawned two.
        let dir = crate::deploy_worker::files::tests::temp_dir("claim-live");
        let f = Files::of(&dir, "lab-1");
        let mut child = fake_worker();
        let worker = Worker { pid: child.id(), started: observe(child.id()).map(|o| o.started) };
        std::fs::write(&f.pid, worker.line()).unwrap();
        let err = claim(&f).unwrap_err().to_string();
        assert!(err.contains("already in progress"), "{err}");
        assert_eq!(read_worker(&f.pid), Some(worker), "the running worker's pidfile is kept");
        assert_eq!(running_in(&dir), ["lab-1"]);
        child.kill().unwrap();
        child.wait().unwrap();
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_just_claimed_pidfile_is_not_swept_as_stale() {
        let dir = crate::deploy_worker::files::tests::temp_dir("claimed");
        let f = Files::of(&dir, "lab-1");
        drop(claim(&f).unwrap());
        assert!(running_in(&dir).is_empty());
        assert!(f.pid.exists());
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn only_the_workers_own_pidfile_is_removed() {
        let dir = crate::deploy_worker::files::tests::temp_dir("own-pid");
        let p = dir.join("lab-1.pid");
        std::fs::write(&p, "200 1").unwrap();
        remove_pidfile_of(&p, 100);
        assert!(p.exists(), "a newer worker's pidfile stays");
        remove_pidfile_of(&p, 200);
        assert!(!p.exists());
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn stale_pidfiles_are_cleared_and_only_live_workers_listed() {
        let dir = crate::deploy_worker::files::tests::temp_dir("running");
        let f = Files::of(&dir, "lab-1");
        std::fs::write(&f.pid, "4000000000").unwrap();
        std::fs::write(dir.join("lab-2.pid"), "garbage").unwrap();
        std::fs::write(dir.join("lab-3.log"), "not a pidfile").unwrap();
        assert!(running_in(&dir).is_empty());
        assert!(!f.pid.exists() && !dir.join("lab-2.pid").exists());
        assert!(dir.join("lab-3.log").exists());
        assert!(jobs_in(&dir).is_empty());
        assert!(running_in(&dir.join("missing")).is_empty());
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn killing_a_worker_that_is_gone_just_drops_its_pidfile() {
        let dir = crate::deploy_worker::files::tests::temp_dir("kill");
        let f = Files::of(&dir, "lab-1");
        std::fs::write(&f.pid, "4000000000").unwrap();
        std::fs::write(&f.job, "{}").unwrap();
        kill_files(&f);
        assert!(!f.pid.exists());
        assert!(!f.job.exists(), "its job (and launch token) goes with it");
        kill_files(&f);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn the_job_and_its_log_are_written_for_this_user_only() {
        let dir = crate::deploy_worker::files::tests::temp_dir("private");
        let f = Files::of(&dir, "lab-1");
        // Left by an older version, readable by everyone.
        std::fs::write(&f.log, "old run").unwrap();
        let job = Job::on_lab("lab-1", super::super::Op::Launch);
        drop(write_job(&f, &job).unwrap());
        assert_eq!(serde_json::from_slice::<Job>(&std::fs::read(&f.job).unwrap()).unwrap(), job);
        assert_eq!(std::fs::read_to_string(&f.log).unwrap(), "", "a fresh log");
        #[cfg(unix)]
        {
            assert_eq!(crate::private_fs::tests::mode(&f.job), 0o600);
            assert_eq!(crate::private_fs::tests::mode(&f.log), 0o600);
        }
        std::fs::remove_dir_all(dir).ok();
    }

    /// A process that looks like a live worker (`… deploy --job …`), leading its own group.
    #[cfg(unix)]
    fn fake_worker_group() -> std::process::Child {
        use std::os::unix::process::CommandExt;
        exec_done(std::process::Command::new("sh").args(["-c", "sleep 30; exit 0", "deploy", "--job", "x.json"]).process_group(0).spawn().unwrap())
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_live_worker_is_listed_refuses_a_second_run_and_is_stopped() {
        let t = crate::test_app::TestApp::new();
        let app = t.handle();
        let mut child = fake_worker_group();
        let job = Job::on_lab("lab-1", super::super::Op::Resume { runtime: crate::runtime::Runtime::Vm });
        let f = files::of(app, &job.key()).unwrap();
        std::fs::write(&f.pid, pidfile_line(child.id())).unwrap();
        std::fs::write(&f.job, serde_json::to_vec(&job).unwrap()).unwrap();
        std::fs::write(&f.log, "Resuming the lab…\n").unwrap();
        assert_eq!(running(app), ["lab-1"]);
        let jobs = running_jobs(app);
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].0, job);
        // A second operation on the lab is refused while this one runs.
        let err = super::super::run_job(app, job.clone(), tauri::ipc::Channel::new(|_| Ok(()))).await.unwrap_err();
        assert_eq!(err.code(), crate::error::Code::AlreadyInProgress);
        // Stopping it ends its process group, and only that (regression: procps' `kill -TERM
        // -<pid>` signalled every process of the user), and drops its pidfile.
        let mut bystander = std::process::Command::new("sleep").arg("30").spawn().unwrap();
        let waiter = std::thread::spawn(move || child.wait());
        kill_and_wait(app, "lab-1").await;
        let status = waiter.join().unwrap().unwrap();
        assert!(!status.success(), "the worker was signalled");
        assert!(bystander.try_wait().unwrap().is_none(), "a process outside its group is left alone");
        let _ = bystander.kill();
        let _ = bystander.wait();
        assert!(!f.pid.exists());
        assert!(running(app).is_empty());
        // Nothing left to stop.
        kill_and_wait(app, "lab-1").await;
        kill(app, "lab-2");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_stop_waits_for_a_claimed_pidfile_to_name_its_worker() {
        // Regression: `kill_and_wait` returned at once while the pidfile was claimed but still
        // empty, so the worker spawned right after went on to take the lab and run its job.
        let t = crate::test_app::TestApp::new();
        let app = t.handle();
        let f = files::of(app, "lab-1").unwrap();
        std::fs::write(&f.pid, "").unwrap();
        let mut child = fake_worker_group();
        let line = pidfile_line(child.id());
        let pid = f.pid.clone();
        let spawner = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(400));
            std::fs::write(&pid, line).unwrap();
        });
        let waiter = std::thread::spawn(move || child.wait());
        kill_and_wait(app, "lab-1").await;
        spawner.join().unwrap();
        assert!(!waiter.join().unwrap().unwrap().success(), "the worker was found and stopped");
        assert!(!f.pid.exists());
    }

    #[tokio::test]
    async fn a_pidfile_left_empty_is_waited_for_only_briefly() {
        let t = crate::test_app::TestApp::new();
        let f = files::of(t.handle(), "lab-2").unwrap();
        std::fs::write(&f.pid, "").unwrap();
        let waited = tokio::time::timeout(Duration::from_secs(10), kill_and_wait(t.handle(), "lab-2")).await;
        assert!(waited.is_ok());
    }

    #[tokio::test]
    async fn a_dead_pid_is_gone_at_once() {
        assert!(gone_within(&Worker { pid: 4_000_000_000, started: None }, 3).await);
    }

    #[test]
    fn a_foreign_program_mentioning_deploy_and_job_is_not_a_worker() {
        // With a legacy pidfile (no start time) and a recycled id, `kubectl ... deploy --job x`
        // (or any tool with those words) keeps the lab "in progress" and gets SIGTERMed by Stop.
        // The OS named its executable, and it isn't this app's; that is enough to tell.
        // Fails with: assertion failed: !is_live_worker(...).
        // Suggested fix: when `seen.exe` is known and differs from `this_exe`, return false
        // before looking at the command line.
        let legacy = Worker { pid: 4242, started: None };
        let foreign = Observed { started: 1, zombie: false, cmdline: "/usr/bin/kubectl rollout deploy --job x".into(), exe: Some("kubectl".into()) };
        assert!(!is_live_worker(&legacy, Some(&foreign), Some("cyberctf-desktop")));
    }

    #[test]
    fn a_pidfile_naming_pid_zero_or_one_never_signals_anything() {
        // A half-written or corrupt pidfile may read as 0 or 1; signalling that group would hit
        // this very process (0) or init (1). Both are dropped as stale, nothing is sent.
        let dir = crate::deploy_worker::files::tests::temp_dir("kill-zero");
        let f = Files::of(&dir, "lab-1");
        for text in ["0", "1", "0 1760000000"] {
            std::fs::write(&f.pid, text).unwrap();
            kill_files(&f);
            assert!(!f.pid.exists(), "{text}: the stale pidfile is dropped");
        }
        assert!(running_in(&dir).is_empty());
        std::fs::remove_dir_all(dir).ok();
    }

    #[cfg(unix)]
    #[test]
    fn a_half_written_start_time_never_names_the_live_worker() {
        // The pidfile is written in one `write_all`, but a reader can still see a prefix of it.
        // A truncated start time differs from the real one, so the worker reads as not alive:
        // the pidfile is then stale and replaced, never a second worker killed by mistake.
        let mut child = fake_worker();
        let pid = child.id();
        let started = observe(pid).unwrap().started;
        let truncated = started.to_string();
        let truncated = &truncated[..truncated.len() - 2];
        let w = Worker::parse(&format!("{pid} {truncated}")).unwrap();
        assert!(!alive(&w), "{w:?} is not the worker started at {started}");
        // A bare pid prefix is some other (or no) process.
        let prefix: u32 = pid.to_string()[..1].parse().unwrap();
        assert!(!alive(&Worker { pid: prefix, started: None }));
        child.kill().unwrap();
        child.wait().unwrap();
    }

    #[test]
    fn the_sweep_keeps_an_empty_pidfile_but_drops_a_whitespace_only_one() {
        // `claim` creates the pidfile empty before the worker is spawned; a sweep in that window
        // must leave it. One holding only whitespace was never a claim: stale.
        let dir = crate::deploy_worker::files::tests::temp_dir("sweep-empty");
        std::fs::write(dir.join("lab-1.pid"), "").unwrap();
        std::fs::write(dir.join("lab-2.pid"), " \n").unwrap();
        assert!(running_in(&dir).is_empty());
        assert!(dir.join("lab-1.pid").exists());
        assert!(!dir.join("lab-2.pid").exists());
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_running_job_with_a_corrupt_job_file_is_listed_as_running_but_not_as_a_job() {
        let dir = crate::deploy_worker::files::tests::temp_dir("jobs-corrupt");
        let f = Files::of(&dir, "lab-1");
        // This test process is alive, but not a worker: not listed at all.
        std::fs::write(&f.pid, std::process::id().to_string()).unwrap();
        std::fs::write(&f.job, "{not json").unwrap();
        assert!(running_in(&dir).is_empty());
        assert!(jobs_in(&dir).is_empty());
        std::fs::remove_dir_all(dir).ok();
    }
}

#[cfg(test)]
mod proptests;
