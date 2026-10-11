//! Worker processes: starting one detached, telling a live worker from a stale pidfile, listing
//! the running ones, and stopping one with everything it started.

use std::path::Path;
use std::time::Duration;

use crate::AppHandle;

use super::Job;
use super::files::{self, Files, read_pid};
use crate::error::{Error, Result};

/// A worker just started: its files and its process id.
pub struct Spawned {
    pub files: Files,
    pub pid: u32,
}

/// Starts the worker for `job`, detached from this process. `Err` when it can't be started; the
/// caller then deploys in-process as a fallback.
pub fn spawn(app: &AppHandle, job: &Job) -> Result<Spawned> {
    crate::runtime::validate_id(&job.lab_id)?;
    let f = files::of(app, &job.key())?;
    // A fresh run: no verdict or log left from an earlier one.
    let _ = std::fs::remove_file(&f.status);
    std::fs::write(&f.job, serde_json::to_vec(job).map_err(|e| Error::Invalid(e.to_string()))?).map_err(Error::Io)?;
    let log = std::fs::File::create(&f.log).map_err(Error::Io)?;
    let exe = std::env::current_exe().map_err(Error::Io)?;
    let mut cmd = std::process::Command::new(exe);
    cmd.arg("deploy").arg("--job").arg(&f.job).stdin(std::process::Stdio::null()).stdout(log.try_clone().map_err(Error::Io)?).stderr(log);
    detach(&mut cmd);
    let mut child = cmd.spawn().map_err(Error::Io)?;
    let pid = child.id();
    std::fs::write(&f.pid, pid.to_string()).map_err(Error::Io)?;
    // Reaped when it exits: unreaped, a finished worker stays a zombie for as long as the app
    // runs, and a zombie still answers `kill -0`. Its pidfile goes with it: a worker that
    // finished before the line above wrote the pidfile would otherwise leave one behind.
    let pidfile = f.pid.clone();
    std::thread::spawn(move || {
        let _ = child.wait();
        if read_pid(&pidfile) == Some(pid) {
            let _ = std::fs::remove_file(&pidfile);
        }
    });
    Ok(Spawned { files: f, pid })
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

/// Whether `pid` is a live deploy worker: running (not a zombie) and this app's `deploy --job`
/// process, not another program that got the number of a worker long gone.
pub fn alive(pid: u32) -> bool {
    #[cfg(target_os = "linux")]
    {
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).unwrap_or_default();
        let cmdline = std::fs::read(format!("/proc/{pid}/cmdline")).unwrap_or_default();
        proc_state(&stat).is_some_and(|c| c != 'Z' && c != 'X') && is_worker(&String::from_utf8_lossy(&cmdline).replace('\0', " "))
    }
    #[cfg(all(unix, not(target_os = "linux")))]
    {
        std::process::Command::new("ps")
            .args(["-o", "stat=,command=", "-p", &pid.to_string()])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .is_some_and(|l| !l.starts_with('Z') && is_worker(&l))
    }
    #[cfg(windows)]
    {
        crate::exec::headless_std(std::process::Command::new("tasklist").args(["/FI", &format!("PID eq {pid}"), "/NH"]))
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).contains(&pid.to_string()))
            .unwrap_or(false)
    }
}

/// The state letter in a `/proc/<pid>/stat` line. It follows the command name, which is in
/// parentheses and may hold spaces.
#[cfg(any(target_os = "linux", test))]
fn proc_state(stat: &str) -> Option<char> {
    stat.rsplit_once(") ").and_then(|(_, rest)| rest.chars().next())
}

/// A deploy worker's command line: `<app> deploy --job <file>`. (Windows checks the pid alone.)
#[cfg(any(unix, test))]
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
        match read_pid(&p) {
            Some(pid) if alive(pid) => out.push(key),
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
    if let Some(pid) = f.pid().filter(|p| alive(*p)) {
        #[cfg(unix)]
        signal_group(pid, libc::SIGTERM);
        #[cfg(windows)]
        {
            let _ = crate::exec::headless_std(std::process::Command::new("taskkill").args(["/PID", &pid.to_string(), "/T", "/F"])).status();
        }
    }
    let _ = std::fs::remove_file(&f.pid);
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

/// [`kill`], then waits until the worker is gone (SIGKILL to its group after 20 s), so what
/// follows (a teardown) never races a `compose up` or `vagrant up` that is still creating what
/// it removes.
pub async fn kill_and_wait(app: &AppHandle, key: &str) {
    let Ok(f) = files::of(app, key) else { return };
    let Some(pid) = f.pid().filter(|p| alive(*p)) else { return };
    kill(app, key);
    if gone_within(pid, 40).await {
        return;
    }
    #[cfg(unix)]
    signal_group(pid, libc::SIGKILL);
    gone_within(pid, 10).await;
}

/// Whether `pid` stops being a live worker within `checks` half-second checks.
async fn gone_within(pid: u32, checks: u32) -> bool {
    for _ in 0..checks {
        if !alive(pid) {
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

    #[test]
    fn the_process_state_follows_a_command_name_with_spaces() {
        assert_eq!(proc_state("1234 (cyberctf-desktop) S 1 1234"), Some('S'));
        assert_eq!(proc_state("1234 (Web Content) Z 1 1234"), Some('Z'));
        assert_eq!(proc_state(""), None);
    }

    #[test]
    fn only_pidfiles_name_a_worker() {
        assert_eq!(pidfile_key(Path::new("/d/lab-1.pid")).as_deref(), Some("lab-1"));
        assert_eq!(pidfile_key(Path::new("/d/lab-1.attack.pid")).as_deref(), Some("lab-1.attack"));
        assert_eq!(pidfile_key(Path::new("/d/lab-1.log")), None);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_finished_or_foreign_process_is_not_alive() {
        // This test binary is alive but isn't a worker; a reaped child is gone.
        assert!(!alive(std::process::id()));
        let mut child = std::process::Command::new("true").spawn().unwrap();
        let pid = child.id();
        child.wait().unwrap();
        assert!(!alive(pid));
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
    fn fake_worker() -> std::process::Child {
        use std::os::unix::process::CommandExt;
        std::process::Command::new("sh").args(["-c", "sleep 30; exit 0", "deploy", "--job", "x.json"]).process_group(0).spawn().unwrap()
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_live_worker_is_listed_refuses_a_second_run_and_is_stopped() {
        let t = crate::test_app::TestApp::new();
        let app = t.handle();
        let mut child = fake_worker();
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
        assert!(gone_within(4_000_000_000, 3).await);
    }
}
