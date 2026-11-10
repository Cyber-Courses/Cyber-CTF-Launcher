//! A worker's files under `<app data>/deploys/`, keyed by the lab.

use std::path::{Path, PathBuf};

use super::Job;
use super::process::{Worker, alive};
use crate::error::{Error, Result};
use crate::platform::AppDirs;

/// A worker's files.
pub struct Files {
    /// The job it runs (`<key>.json`).
    pub job: PathBuf,
    /// What it says, also its stdout/stderr (`<key>.log`).
    pub log: PathBuf,
    /// The log of the run before (`<key>.previous.log`), kept when the next one starts.
    pub previous_log: PathBuf,
    /// Its process id, while it runs (`<key>.pid`).
    pub pid: PathBuf,
    /// `ok` or `error: ...`, written last (`<key>.status`).
    pub status: PathBuf,
    /// The verdict of the last operation that brought the lab up (a launch or a resume) alone,
    /// in the same form (`<key>.launch-status`): the other operations (a pause, a provisioning
    /// run) write `status` too, and their failure says nothing about whether the lab came up.
    pub launch_status: PathBuf,
}

impl Files {
    pub fn of(dir: &Path, key: &str) -> Files {
        Files {
            job: dir.join(format!("{key}.json")),
            log: dir.join(format!("{key}.log")),
            previous_log: dir.join(format!("{key}.previous.log")),
            pid: dir.join(format!("{key}.pid")),
            status: dir.join(format!("{key}.status")),
            launch_status: dir.join(format!("{key}.launch-status")),
        }
    }

    /// The worker its pidfile names, if any.
    pub fn worker(&self) -> Option<Worker> {
        read_worker(&self.pid)
    }

    /// Whether this lab's last finished launch or resume failed (none still running, a launch
    /// status whose [`verdict`] is an error). A failed pause or provisioning run doesn't count.
    pub fn failed(&self) -> bool {
        if self.worker().is_some_and(|w| alive(&w)) {
            return false;
        }
        std::fs::read_to_string(&self.launch_status).is_ok_and(|s| verdict(&s).is_err())
    }
}

/// What a status file says for a result, and the worker's exit code that goes with it: `ok`, or
/// the error as the webview gets it (`error: {"code":…,"message":…}`), its code read back by
/// [`verdict`].
pub(super) fn status_line<T>(result: &Result<T>) -> (String, i32) {
    match result {
        Ok(_) => ("ok".to_string(), 0),
        Err(e) => (format!("error: {}", serde_json::to_string(&e.to_wire()).unwrap_or_else(|_| e.to_string())), 1),
    }
}

/// The verdict in a status file, as [`status_line`] wrote it: `Ok` for `ok`, else the error the
/// worker wrote, with its code, or a bare message (a worker of an older version). The one reader
/// of the format: following a worker and asking whether its lab's last launch failed agree.
pub fn verdict(status: &str) -> Result<()> {
    let s = status.trim();
    if s == "ok" {
        return Ok(());
    }
    let body = s.strip_prefix("error: ").unwrap_or(s);
    Err(serde_json::from_str::<crate::error::Wire>(body).map_or_else(|_| Error::Invalid(body.to_string()), Error::from))
}

/// The workers' folder (`runtime::deploys_dir`).
pub fn dir(dirs: &AppDirs) -> Result<PathBuf> {
    crate::runtime::deploys_dir(dirs)
}

/// The files of `key`'s worker.
pub fn of(dirs: &AppDirs, key: &str) -> Result<Files> {
    Ok(Files::of(&dir(dirs)?, key))
}

/// The worker a pidfile names: `<pid> <start time>` (or a bare `<pid>`, from older versions).
pub fn read_worker(path: &Path) -> Option<Worker> {
    Worker::parse(&std::fs::read_to_string(path).ok()?)
}

/// Whether the lab's last finished launch failed (its worker wrote `error: …`). A run still in
/// progress, none at all, or another operation that failed, isn't a failed launch.
pub fn last_deploy_failed(dirs: &AppDirs, lab_id: &str) -> bool {
    let Ok(f) = of(dirs, lab_id) else { return false };
    f.failed()
}

/// The log of a lab's latest deploy so far, for an app that (re)attaches to a running worker.
pub fn log_so_far(dirs: &AppDirs, lab_id: &str) -> Result<String> {
    crate::runtime::validate_id(lab_id)?;
    let f = of(dirs, lab_id)?;
    Ok(std::fs::read_to_string(&f.log).unwrap_or_default())
}

/// Removes the file at `path`; one already gone is no failure.
pub(super) fn remove(path: &Path) -> std::io::Result<()> {
    match std::fs::remove_file(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        removed => removed,
    }
}

/// Clears the verdicts an earlier run left (its status, and for an operation that brings the lab
/// up (`launch`) its launch status), before a new run starts: the app reads any status it finds
/// as the new run's. One that can't be removed fails the start, which would otherwise report the
/// earlier run's verdict at once.
pub(super) fn clear_verdicts(f: &Files, launch: bool) -> Result<()> {
    let launch_status = launch.then_some(&f.launch_status);
    for path in std::iter::once(&f.status).chain(launch_status) {
        remove(path).map_err(|e| Error::Invalid(format!("the last run's verdict {} can't be removed: {e}", path.display())))?;
    }
    Ok(())
}

/// Removes a job file, which carries the lab's launch token: one already gone is fine, any other
/// failure is logged (the token stays on disk).
pub(super) fn remove_job(path: &Path) {
    if let Err(e) = remove(path) {
        log::warn!("couldn't remove the job file {}, which holds a launch token: {e}", path.display());
    }
}

/// Writes the job for its worker and opens its fresh log, both owner-only: the job carries the
/// launch token (the worker removes it when the job ends), the log what the tools printed. The
/// last run's log is kept beside it ([`Files::previous_log`]): a start again right after a failed
/// one emptied the only log that said why the first failed.
pub(super) fn write_job(f: &Files, job: &Job) -> Result<std::fs::File> {
    crate::platform::private_fs::write(&f.job, serde_json::to_vec(job).map_err(|e| Error::Invalid(e.to_string()))?).map_err(Error::Io)?;
    keep_last_log(f);
    crate::platform::private_fs::create(&f.log).map_err(Error::Io)
}

/// Moves the last run's log, if any, to [`Files::previous_log`] (owner-only, as the log is).
/// Best effort: one that can't be kept is logged, and the new run starts all the same.
fn keep_last_log(f: &Files) {
    let kept = match std::fs::rename(&f.log, &f.previous_log) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
        // An older version's log was readable by everyone.
        moved => moved.and_then(|()| crate::platform::private_fs::append(&f.previous_log).map(drop)),
    };
    if let Err(e) = kept {
        log::warn!("couldn't keep the last run's log {} as {}: {e}", f.log.display(), f.previous_log.display());
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::testing::temp_dir;

    #[test]
    fn a_workers_files_share_its_key() {
        let f = Files::of(Path::new("/data/deploys"), "lab-1.attack");
        assert_eq!(f.job, Path::new("/data/deploys/lab-1.attack.json"));
        assert_eq!(f.log, Path::new("/data/deploys/lab-1.attack.log"));
        assert_eq!(f.previous_log, Path::new("/data/deploys/lab-1.attack.previous.log"));
        assert_eq!(f.pid, Path::new("/data/deploys/lab-1.attack.pid"));
        assert_eq!(f.status, Path::new("/data/deploys/lab-1.attack.status"));
        assert_eq!(f.launch_status, Path::new("/data/deploys/lab-1.attack.launch-status"));
    }

    #[test]
    fn pidfiles_read_a_number_or_nothing() {
        let dir = std::env::temp_dir().join(format!("cyberctf-pid-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("x.pid");
        std::fs::write(&p, "4242\n").unwrap();
        assert_eq!(read_worker(&p), Some(Worker { pid: 4242, started: None }));
        std::fs::write(&p, "4242 1760000000\n").unwrap();
        assert_eq!(read_worker(&p), Some(Worker { pid: 4242, started: Some(1_760_000_000) }));
        std::fs::write(&p, "nope").unwrap();
        assert_eq!(read_worker(&p), None);
        std::fs::write(&p, "").unwrap();
        assert_eq!(read_worker(&p), None);
        assert_eq!(read_worker(&dir.join("missing.pid")), None);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn only_a_finished_error_status_is_a_failed_deploy() {
        let dir = temp_dir("failed");
        let f = Files::of(&dir, "lab-1");
        assert!(!f.failed(), "no run at all");
        std::fs::write(&f.launch_status, "ok\n").unwrap();
        assert!(!f.failed());
        std::fs::write(&f.launch_status, "  error: Docker isn't running").unwrap();
        assert!(f.failed());
        // A pidfile naming no live worker doesn't hide the verdict.
        std::fs::write(&f.pid, "4000000000").unwrap();
        assert!(f.failed());
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_failed_operation_after_a_good_launch_is_not_a_failed_launch() {
        // Regression: every operation (pause, resume, provisioning) wrote the one status file
        // the launch verdict was read from, so a failed provisioning run made a lab running on a
        // server read as not running.
        let dir = temp_dir("failed-op");
        let f = Files::of(&dir, "lab-1");
        std::fs::write(&f.launch_status, "ok").unwrap();
        std::fs::write(&f.status, "error: provisioning failed").unwrap();
        assert!(!f.failed());
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn pidfiles_with_odd_whitespace_bom_or_partial_writes() {
        let dir = temp_dir("pid-odd");
        let p = dir.join("x.pid");
        for (text, want) in [
            (" 4242 \r\n", Some(Worker { pid: 4242, started: None })),
            ("4242\t1760000000\n", Some(Worker { pid: 4242, started: Some(1_760_000_000) })),
            ("4242 176", Some(Worker { pid: 4242, started: Some(176) })),
            ("4242 x", Some(Worker { pid: 4242, started: None })),
            ("42", Some(Worker { pid: 42, started: None })),
            ("-1", None),
            ("4294967296", None),
            ("\u{feff}4242", None),
            ("4242.0", None),
        ] {
            std::fs::write(&p, text).unwrap();
            assert_eq!(read_worker(&p), want, "{text:?}");
        }
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_failed_verdict_is_read_however_it_is_written() {
        // Regression: the launch status was read by a check of its own (`starts_with("error")`),
        // while following the worker read it with `verdict`: a bare message (an older worker's
        // verdict) failed the run it followed, but its lab read as not failed, so as running.
        let dir = temp_dir("failed-forms");
        let f = Files::of(&dir, "lab-1");
        for (text, want) in [
            ("ok", false),
            (" ok\r\n", false),
            ("error: x", true),
            ("\r\n\terror", true),
            ("Docker isn't running", true),
            ("Error: x", true),
            ("ok\nerror: later", true),
        ] {
            std::fs::write(&f.launch_status, text).unwrap();
            assert_eq!(f.failed(), want, "{text:?}");
            assert_eq!(verdict(text).is_err(), want, "{text:?}: the two agree");
        }
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_verdict_that_cant_be_cleared_fails_the_start() {
        // Regression: a failed removal of the last run's status was dropped, and the new run then
        // reported that run's verdict at once.
        let dir = temp_dir("clear-verdicts");
        let f = Files::of(&dir, "lab-1");
        std::fs::write(&f.status, "error: the last run").unwrap();
        std::fs::write(&f.launch_status, "error: the last launch").unwrap();
        clear_verdicts(&f, false).unwrap();
        assert!(!f.status.exists() && f.launch_status.exists(), "a pause keeps the launch's verdict");
        clear_verdicts(&f, true).unwrap();
        assert!(!f.launch_status.exists());
        clear_verdicts(&f, true).unwrap();
        // A status that can't be removed (here a folder in its place).
        std::fs::create_dir_all(f.status.join("held")).unwrap();
        let err = clear_verdicts(&f, false).unwrap_err().to_string();
        assert!(err.starts_with("the last run's verdict ") && err.contains("can't be removed"), "{err}");
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_job_file_that_cant_be_removed_is_logged() {
        let dir = temp_dir("remove-job");
        let job = dir.join("lab-1.json");
        crate::testing::logs::take_warnings();
        remove_job(&job);
        assert!(crate::testing::logs::take_warnings().is_empty(), "already gone");
        std::fs::write(&job, "{}").unwrap();
        remove_job(&job);
        assert!(!job.exists());
        std::fs::create_dir_all(job.join("held")).unwrap();
        remove_job(&job);
        let logged = crate::testing::logs::take_warnings();
        assert!(logged.iter().any(|w| w.starts_with("couldn't remove the job file ") && w.contains("launch token")), "{logged:?}");
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn verdict_reads_ok_and_errors() {
        assert!(verdict("ok\n").is_ok());
        let err = verdict("error: Docker isn't running").unwrap_err();
        assert!(format!("{err:?}").contains("Docker isn't running"));
        // The worker's error keeps its code across the process boundary.
        let err = verdict(r#"error: {"code":"portInUse","args":{"port":8080},"message":"Host port 8080 is taken"}"#).unwrap_err();
        assert_eq!(err.code(), crate::error::Code::PortInUse { port: 8080, holder: None });
        assert_eq!(err.to_string(), "Host port 8080 is taken");
        // A bare message is still an error, kept whole.
        assert_eq!(verdict("something else").unwrap_err().to_string(), "something else");
    }

    #[test]
    fn the_log_so_far_needs_a_valid_id_and_is_empty_before_any_deploy() {
        let t = crate::testing::test_app::TestApp::new();
        assert!(log_so_far(&t.dirs(), "../x").unwrap_err().to_string().starts_with("invalid lab id"));
        assert_eq!(log_so_far(&t.dirs(), "never-deployed").unwrap(), "");
        assert!(!last_deploy_failed(&t.dirs(), "never-deployed"));
        let f = of(&t.dirs(), "lab-1").unwrap();
        std::fs::write(&f.log, "line\n").unwrap();
        std::fs::write(&f.launch_status, "error: boom").unwrap();
        assert_eq!(log_so_far(&t.dirs(), "lab-1").unwrap(), "line\n");
        assert!(last_deploy_failed(&t.dirs(), "lab-1"));
        assert!(!last_deploy_failed(&t.dirs(), "../x"), "a bad id is never a failed deploy");
    }

    #[test]
    fn the_job_and_its_log_are_written_for_this_user_only() {
        let dir = crate::testing::temp_dir("private");
        let f = Files::of(&dir, "lab-1");
        // Left by an older version, readable by everyone.
        std::fs::write(&f.log, "old run").unwrap();
        let job = Job::on_lab("lab-1", super::super::Op::Launch);
        drop(write_job(&f, &job).unwrap());
        assert_eq!(serde_json::from_slice::<Job>(&std::fs::read(&f.job).unwrap()).unwrap(), job);
        assert_eq!(std::fs::read_to_string(&f.log).unwrap(), "", "a fresh log");
        #[cfg(unix)]
        {
            assert_eq!(crate::platform::private_fs::tests::mode(&f.job), 0o600);
            assert_eq!(crate::platform::private_fs::tests::mode(&f.log), 0o600);
            assert_eq!(crate::platform::private_fs::tests::mode(&f.previous_log), 0o600);
        }
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_new_run_keeps_the_last_runs_log() {
        // Regression: a start again right after one whose worker died emptied the only log that
        // could say why (its stderr), so the first failure could never be explained.
        let dir = crate::testing::temp_dir("previous-log");
        let f = Files::of(&dir, "lab-1");
        let job = Job::on_lab("lab-1", super::super::Op::Launch);
        drop(write_job(&f, &job).unwrap());
        assert!(!f.previous_log.exists(), "no run before the first");
        std::fs::write(&f.log, "Downloading CyberCTF/web@abc\nthread 'main' has overflowed its stack\n").unwrap();
        drop(write_job(&f, &job).unwrap());
        assert_eq!(std::fs::read_to_string(&f.previous_log).unwrap(), "Downloading CyberCTF/web@abc\nthread 'main' has overflowed its stack\n");
        assert_eq!(std::fs::read_to_string(&f.log).unwrap(), "");
        // One run back only: the run before that one goes.
        std::fs::write(&f.log, "second run\n").unwrap();
        drop(write_job(&f, &job).unwrap());
        assert_eq!(std::fs::read_to_string(&f.previous_log).unwrap(), "second run\n");
        std::fs::remove_dir_all(dir).ok();
    }
}
