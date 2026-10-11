//! The worker process itself: a headless Tauri app that runs one job and exits.

use std::io::Write as _;
use std::path::PathBuf;

use tauri::AppHandle;

use super::progress::FAILED_MARK;
use super::{Job, Op, execute, files};

/// The worker's entry point, in place of the app: a headless Tauri app on the same data dir
/// (so lab, key and server paths resolve the same), the deploy, a status file, then exit.
pub fn worker_main(job_path: PathBuf, mut context: tauri::Context) {
    let Some(job) = read_job(&job_path) else {
        eprintln!("deploy worker: unreadable job file {}", job_path.display());
        std::process::exit(2);
    };
    // No window: this is a background process.
    context.config_mut().app.windows.clear();
    tauri::Builder::default()
        .setup(move |app| {
            crate::shared_folder::init(app.handle());
            // A background process, not an app: on macOS keep it out of the Dock, the app
            // switcher and Apple Events aimed at "Cyber CTF".
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Prohibited);
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let code = run_logged(&handle, job).await;
                handle.exit(code);
            });
            // Stopped (the app's Stop sends SIGTERM to the worker's process group): the timed
            // tools it runs (status probes) sit in groups of their own, out of that signal's
            // reach; end them too before going.
            #[cfg(unix)]
            tauri::async_runtime::spawn(async {
                if let Ok(mut term) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
                    term.recv().await;
                    crate::exec::kill_live_tools();
                    std::process::exit(143);
                }
            });
            Ok(())
        })
        .build(context)
        .expect("error while starting the deploy worker")
        .run(|_app, event| {
            // Only the deploy ends the worker (`handle.exit(code)` carries its code). A quit from
            // outside (a script or tool quitting "Cyber CTF") reached it once and killed a deploy
            // mid-way, orphaning vagrant with no status written.
            if let tauri::RunEvent::ExitRequested { code: None, api, .. } = &event {
                api.prevent_exit();
            }
            // macOS still lists the worker as a running "Cyber CTF": with the app itself closed,
            // opening Cyber CTF (Dock, Finder, `open`) reached the worker, which has no window,
            // and nothing opened. Hand such a request to the real app, and stay in the background
            // (an AppleScript "activate" had turned the worker into a foreground app).
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Reopen { .. } = &event {
                let _ = _app.set_activation_policy(tauri::ActivationPolicy::Prohibited);
                open_main_app();
            }
        });
}

fn read_job(path: &std::path::Path) -> Option<Job> {
    std::fs::read(path).ok().and_then(|b| serde_json::from_slice(&b).ok())
}

/// Starts the app itself (a new instance; if one is already open, it is brought forward).
#[cfg(target_os = "macos")]
fn open_main_app() {
    // Contents/MacOS/<binary> -> the .app bundle.
    let bundle = std::env::current_exe().ok().and_then(|exe| exe.ancestors().nth(3).map(std::path::Path::to_path_buf));
    match bundle.filter(|b| b.extension().is_some_and(|e| e == "app")) {
        Some(b) => {
            let _ = std::process::Command::new("/usr/bin/open").arg("-n").arg(&b).spawn();
        }
        // Not in a bundle (a dev build): run the binary without the worker arguments.
        None => {
            if let Ok(exe) = std::env::current_exe() {
                let _ = std::process::Command::new(exe).spawn();
            }
        }
    }
}

/// Runs the job, logging to its file (also this process's stdout/stderr, for panics), and leaves
/// the verdict in the status file. Returns the process's exit code.
async fn run_logged(app: &AppHandle, job: Job) -> i32 {
    let Ok(f) = files::of(app, &job.key()) else { return 2 };
    let sink = std::sync::Mutex::new(std::fs::OpenOptions::new().append(true).create(true).open(&f.log).ok());
    let log = |line: String| {
        if let Ok(mut guard) = sink.lock()
            && let Some(file) = guard.as_mut()
        {
            let _ = writeln!(file, "{line}");
        }
    };
    let launch = job.op == Op::Launch;
    // The lab is this worker's for the whole job (download and install included): the app's
    // own operations on it (a stop, the expiry reaper) wait or step aside meanwhile. The attack
    // VM runs beside the lab's operations, without it.
    let _lab = match job.op {
        Op::AttackVm { .. } => None,
        _ => match super::lock_path(app, &job.lab_id) {
            Ok(path) => super::lock::acquire(&path).await.map_err(|e| log::warn!("deploy worker: no lab lock ({e})")).ok(),
            Err(e) => {
                log::warn!("deploy worker: no lab lock ({e})");
                None
            }
        },
    };
    let result = execute(app, job, &log).await;
    conclude(&f, &result, launch, log)
}

/// The end of a run: the failure in the log, the verdict in the status file (and, for a
/// `launch`, in the launch status too), the pidfile gone. Returns the process's exit code.
fn conclude<T>(f: &files::Files, result: &crate::error::Result<T>, launch: bool, log: impl Fn(String)) -> i32 {
    // The log file says how it ended too, so a page that re-attaches to it after a reload or a
    // relaunch (it has no live verdict) still sees the run failed.
    if let Err(e) = result {
        log(format!("{FAILED_MARK} {e}"));
    }
    let (status, code) = status_line(result);
    if launch {
        let _ = std::fs::write(&f.launch_status, &status);
    }
    let _ = std::fs::write(&f.status, status);
    // Only while it still names this worker: a newer one's pidfile stays.
    super::process::remove_pidfile_of(&f.pid, std::process::id());
    code
}

/// What the status file says for a result, and the exit code that goes with it.
fn status_line<T>(result: &crate::error::Result<T>) -> (String, i32) {
    match result {
        Ok(_) => ("ok".to_string(), 0),
        Err(e) => (format!("error: {e}"), 1),
    }
}

#[cfg(test)]
mod tests {
    use super::{conclude, read_job, status_line};
    use crate::deploy_worker::files::Files;
    use crate::deploy_worker::progress::verdict;
    use crate::deploy_worker::{Job, Op};
    use crate::error::Error;

    #[test]
    fn the_status_line_reads_back_as_the_same_verdict() {
        assert_eq!(status_line(&Ok(())), ("ok".to_string(), 0));
        let (line, code) = status_line::<()>(&Err(Error::Invalid("Docker isn't running".into())));
        assert_eq!((line.as_str(), code), ("error: Docker isn't running", 1));
        assert_eq!(verdict(&line).unwrap_err().to_string(), "Docker isn't running");
    }

    #[test]
    fn a_job_file_reads_back_or_is_refused() {
        let dir = crate::deploy_worker::files::tests::temp_dir("job");
        let path = dir.join("lab-1.json");
        assert!(read_job(&path).is_none());
        let job = Job::on_lab("lab-1", Op::AttackVm { box_name: "a/b".into() });
        std::fs::write(&path, serde_json::to_vec(&job).unwrap()).unwrap();
        assert_eq!(read_job(&path), Some(job));
        std::fs::write(&path, "{").unwrap();
        assert!(read_job(&path).is_none());
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn concluding_writes_the_verdict_and_drops_the_pidfile() {
        let dir = crate::deploy_worker::files::tests::temp_dir("conclude");
        let f = Files::of(&dir, "lab-1");
        std::fs::write(&f.pid, std::process::id().to_string()).unwrap();
        let lines = std::sync::Mutex::new(Vec::new());
        let code = conclude(&f, &Err::<(), _>(Error::Invalid("boom".into())), true, |l| lines.lock().unwrap().push(l));
        assert_eq!(code, 1);
        assert_eq!(lines.lock().unwrap().as_slice(), ["✗ boom"]);
        assert_eq!(std::fs::read_to_string(&f.status).unwrap(), "error: boom");
        assert_eq!(std::fs::read_to_string(&f.launch_status).unwrap(), "error: boom");
        assert!(!f.pid.exists());
        assert_eq!(conclude(&f, &Ok(()), true, |_| panic!("no log on success")), 0);
        assert_eq!(std::fs::read_to_string(&f.status).unwrap(), "ok");
        assert_eq!(std::fs::read_to_string(&f.launch_status).unwrap(), "ok");
        // Another operation leaves the launch's verdict alone.
        conclude(&f, &Err::<(), _>(Error::Invalid("park failed".into())), false, |_| {});
        assert_eq!(std::fs::read_to_string(&f.status).unwrap(), "error: park failed");
        assert_eq!(std::fs::read_to_string(&f.launch_status).unwrap(), "ok");
        std::fs::remove_dir_all(dir).ok();
    }
}
