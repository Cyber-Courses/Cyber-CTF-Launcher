//! A worker's files under `<app data>/deploys/`, keyed by the lab or its attack VM.

use std::path::{Path, PathBuf};

use crate::AppHandle;
use tauri::Manager;

use super::process::alive;
use crate::error::{Error, Result};

/// A worker's files.
pub struct Files {
    /// The job it runs (`<key>.json`).
    pub job: PathBuf,
    /// What it says, also its stdout/stderr (`<key>.log`).
    pub log: PathBuf,
    /// Its process id, while it runs (`<key>.pid`).
    pub pid: PathBuf,
    /// `ok` or `error: ...`, written last (`<key>.status`).
    pub status: PathBuf,
}

impl Files {
    pub fn of(dir: &Path, key: &str) -> Files {
        Files {
            job: dir.join(format!("{key}.json")),
            log: dir.join(format!("{key}.log")),
            pid: dir.join(format!("{key}.pid")),
            status: dir.join(format!("{key}.status")),
        }
    }

    /// The worker's process id, when its pidfile names one.
    pub fn pid(&self) -> Option<u32> {
        read_pid(&self.pid)
    }

    /// Whether this worker's last finished run failed (none still running, `error: …` status).
    pub fn failed(&self) -> bool {
        if self.pid().is_some_and(alive) {
            return false;
        }
        std::fs::read_to_string(&self.status).is_ok_and(|s| s.trim_start().starts_with("error"))
    }
}

/// The workers' folder, created if missing.
pub fn dir(app: &AppHandle) -> Result<PathBuf> {
    let d = app.path().app_data_dir().map_err(|e| Error::Invalid(e.to_string()))?.join("deploys");
    std::fs::create_dir_all(&d).map_err(Error::Io)?;
    Ok(d)
}

/// The files of `key`'s worker.
pub fn of(app: &AppHandle, key: &str) -> Result<Files> {
    Ok(Files::of(&dir(app)?, key))
}

pub fn read_pid(path: &Path) -> Option<u32> {
    std::fs::read_to_string(path).ok().and_then(|s| s.trim().parse().ok())
}

/// Whether the lab's last finished deploy failed (its worker wrote `error: …`). A run still in
/// progress, or none at all, isn't a failure.
pub fn last_deploy_failed(app: &AppHandle, lab_id: &str) -> bool {
    let Ok(f) = of(app, lab_id) else { return false };
    f.failed()
}

/// The log of a lab's latest deploy so far, for an app that (re)attaches to a running worker.
pub fn log_so_far(app: &AppHandle, lab_id: &str) -> Result<String> {
    crate::runtime::validate_id(lab_id)?;
    let f = of(app, lab_id)?;
    Ok(std::fs::read_to_string(&f.log).unwrap_or_default())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn a_workers_files_share_its_key() {
        let f = Files::of(Path::new("/data/deploys"), "lab-1.attack");
        assert_eq!(f.job, Path::new("/data/deploys/lab-1.attack.json"));
        assert_eq!(f.log, Path::new("/data/deploys/lab-1.attack.log"));
        assert_eq!(f.pid, Path::new("/data/deploys/lab-1.attack.pid"));
        assert_eq!(f.status, Path::new("/data/deploys/lab-1.attack.status"));
    }

    #[test]
    fn pidfiles_read_a_number_or_nothing() {
        let dir = std::env::temp_dir().join(format!("cyberctf-pid-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("x.pid");
        std::fs::write(&p, "4242\n").unwrap();
        assert_eq!(read_pid(&p), Some(4242));
        std::fs::write(&p, "nope").unwrap();
        assert_eq!(read_pid(&p), None);
        assert_eq!(read_pid(&dir.join("missing.pid")), None);
        std::fs::remove_dir_all(dir).ok();
    }

    /// A fresh folder under the temp dir for one test.
    pub(in crate::deploy_worker) fn temp_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("cyberctf-{tag}-{}-{}", std::process::id(), rand::random::<u32>()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn only_a_finished_error_status_is_a_failed_deploy() {
        let dir = temp_dir("failed");
        let f = Files::of(&dir, "lab-1");
        assert!(!f.failed(), "no run at all");
        std::fs::write(&f.status, "ok\n").unwrap();
        assert!(!f.failed());
        std::fs::write(&f.status, "  error: Docker isn't running").unwrap();
        assert!(f.failed());
        // A pidfile naming no live worker doesn't hide the verdict.
        std::fs::write(&f.pid, "4000000000").unwrap();
        assert!(f.failed());
        std::fs::remove_dir_all(dir).ok();
    }
}
