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
                return Err(Error::Invalid(super::BUSY.into()));
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
    let started = start(f, job, pidfile);
    if started.is_err() {
        let _ = std::fs::remove_file(&path);
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
    std::fs::write(&f.job, serde_json::to_vec(job).map_err(|e| Error::Invalid(e.to_string()))?).map_err(Error::Io)?;
    let log = std::fs::File::create(&f.log).map_err(Error::Io)?;
    let exe = std::env::current_exe().map_err(Error::Io)?;
    let mut cmd = std::process::Command::new(exe);
    cmd.arg("deploy").arg("--job").arg(&f.job).stdin(std::process::Stdio::null()).stdout(log.try_clone().map_err(Error::Io)?).stderr(log);
    detach(&mut cmd);
    // A UAC prompt it raises (Vagrant on Hyper-V) comes up for the app's window.
    #[cfg(windows)]
    if let Some(hwnd) = crate::exec::owner_window() {
        cmd.env(crate::exec::OWNER_WINDOW_ENV, hwnd.to_string());
    }
    let mut child = cmd.spawn().map_err(Error::Io)?;
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

/// Removes the pidfile at `path` only while it still names the worker `pid`: a newer worker's
/// pidfile is left alone.
pub fn remove_pidfile_of(path: &Path, pid: u32) {
    if read_worker(path).is_some_and(|w| w.pid == pid) {
        let _ = std::fs::remove_file(path);
    }
}

/// Its own process group (and session leader on unix): closing the app's terminal or the app
/// itself sends it nothing, and stopping it later stops its children too.
fn detach(cmd: &mut std::process::Command) {
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
        cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
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
/// On macOS sysinfo first lists every process id, and finds nothing at all when processes were
/// started between its two `proc_listallpids` calls: a live worker then read as gone, and its
/// pidfile was swept as stale. So a process that sysinfo misses but the OS still has is looked
/// at again, a few times.
pub(super) fn observe(pid: u32) -> Option<Observed> {
    for _ in 0..5 {
        if let Some(seen) = observe_once(pid) {
            return Some(seen);
        }
        if !exists(pid) {
            return None;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    None
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

/// [`running_jobs`] for the workers' folder `d`.
fn jobs_in(d: &Path) -> Vec<(Job, Option<String>)> {
    running_in(d)
        .into_iter()
        .filter_map(|key| {
            let f = Files::of(d, &key);
            let job: Job = serde_json::from_slice(&std::fs::read(&f.job).ok()?).ok()?;
            let step = std::fs::read_to_string(&f.log).ok().and_then(|l| super::progress::last_step(&l));
            Some((job, step))
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
/// resources it is creating and records them in its state (2 minutes).
#[cfg(unix)]
const TERRAFORM_GRACE_CHECKS: u32 = 240;

/// [`kill`], then waits until the worker is gone (SIGKILL to its group after 20 s), so what
/// follows (a teardown) never races a `compose up` or `vagrant up` that is still creating what
/// it removes. A worker running Terraform is interrupted first instead: killed mid-apply,
/// Terraform never records what it was creating, and the destroy that follows leaves those
/// resources running (and billing). Interrupted, it stops cleanly with its state saved, the
/// worker ends with it, and the destroy that follows sees everything.
pub async fn kill_and_wait(app: &AppHandle, key: &str) {
    let Ok(f) = files::of(app, key) else { return };
    let Some(worker) = f.worker().filter(alive) else { return };
    #[cfg(unix)]
    if interrupt_terraform(worker.pid) && gone_within(&worker, TERRAFORM_GRACE_CHECKS).await {
        remove_pidfile_of(&f.pid, worker.pid);
        return;
    }
    kill(app, key);
    if gone_within(&worker, 40).await {
        return;
    }
    #[cfg(unix)]
    signal_group(worker.pid, libc::SIGKILL);
    gone_within(&worker, 10).await;
}

/// Sends SIGINT to the Terraform runs in process group `group` (the host binary, or the `docker
/// run` of its container, which hands the signal on): Terraform's own clean stop. Whether one
/// was found.
#[cfg(unix)]
fn interrupt_terraform(group: u32) -> bool {
    let pgrep = |args: &[&str]| std::process::Command::new("pgrep").args(args).output().map(|o| pids(&String::from_utf8_lossy(&o.stdout))).unwrap_or_default();
    let group = group.to_string();
    let mut found = pgrep(&["-g", &group, "-x", "terraform"]);
    found.extend(pgrep(&["-g", &group, "-f", "hashicorp/terraform"]));
    for pid in &found {
        let _ = std::process::Command::new("kill").args(["-INT", &pid.to_string()]).status();
    }
    !found.is_empty()
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
        std::process::Command::new("sh").args(["-c", "sleep 20; :", "deploy", "--job", "x"]).spawn().unwrap()
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
        kill_files(&f);
        assert!(!f.pid.exists());
        kill_files(&f);
        std::fs::remove_dir_all(dir).ok();
    }

    /// A process that looks like a live worker (`… deploy --job …`), leading its own group.
    #[cfg(unix)]
    fn fake_worker_group() -> std::process::Child {
        use std::os::unix::process::CommandExt;
        std::process::Command::new("sh").args(["-c", "sleep 30; exit 0", "deploy", "--job", "x.json"]).process_group(0).spawn().unwrap()
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_live_worker_is_listed_refuses_a_second_run_and_is_stopped() {
        let t = crate::test_app::TestApp::new();
        let app = t.handle();
        let mut child = fake_worker_group();
        let job = Job::on_lab("lab-1", super::super::Op::Resume { runtime: crate::runtime::Runtime::Vm });
        let f = files::of(app, &job.key()).unwrap();
        std::fs::write(&f.pid, child.id().to_string()).unwrap();
        std::fs::write(&f.job, serde_json::to_vec(&job).unwrap()).unwrap();
        std::fs::write(&f.log, "Resuming the lab…\n").unwrap();
        assert_eq!(running(app), ["lab-1"]);
        let jobs = running_jobs(app);
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].0, job);
        // A second operation on the lab is refused while this one runs.
        let err = super::super::run_job(app, job.clone(), tauri::ipc::Channel::new(|_| Ok(()))).await.unwrap_err().to_string();
        assert_eq!(err, "A resume of this lab is already in progress; wait for it to finish.");
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

    #[tokio::test]
    async fn a_dead_pid_is_gone_at_once() {
        assert!(gone_within(&Worker { pid: 4_000_000_000, started: None }, 3).await);
    }
}
