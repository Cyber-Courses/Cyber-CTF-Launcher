//! The worker process itself: a headless Tauri app that runs one job and exits.

use std::io::Write as _;
use std::path::{Path, PathBuf};

use crate::AppHandle;
use tauri::Manager as _;

use super::progress::FAILED_MARK;
use super::{Job, Op, execute, files};

/// The worker's entry point, in place of the app: a headless Tauri app on the same data dir
/// (so lab, key and server paths resolve the same), the deploy, a status file, then exit.
///
/// # Errors
///
/// When Tauri can't build the worker's app.
pub fn worker_main(job_path: &Path, mut context: tauri::Context<crate::AppRuntime>) -> tauri::Result<()> {
    let Some(job) = read_job(job_path) else {
        eprintln!("deploy worker: unreadable job file {}", job_path.display());
        std::process::exit(2);
    };
    #[cfg(unix)]
    let job_file = job_path.to_path_buf();
    // No window: this is a background process.
    context.config_mut().app.windows.clear();
    let log_name = crate::app::logging::worker_log_name(&job.lab_id);
    let app = tauri::Builder::<crate::AppRuntime>::new()
        .plugin(crate::app::logging::plugin(&log_name))
        .setup(move |app| {
            start(app, job);
            #[cfg(unix)]
            end_tools_when_stopped(job_file);
            Ok(())
        })
        .build(context)
        .inspect_err(|e| log::error!("couldn't start the deploy worker: {e}"))?;
    #[cfg(target_os = "macos")]
    app.run(|app, event| {
        keep_running(&event);
        hand_reopen_to_the_app(app, &event);
    });
    #[cfg(not(target_os = "macos"))]
    app.run(|_, event| keep_running(&event));
    Ok(())
}

/// The worker's start: its log and data folder, then the job, and an exit with its code.
fn start(app: &mut tauri::App<crate::AppRuntime>, job: Job) {
    crate::app::logging::log_startup(app.handle(), &format!("deploy worker ({})", job.lab_id));
    if let Ok(dir) = app.path().app_data_dir() {
        crate::shared_folder::init(&dir);
    }
    // A background process, not an app: on macOS keep it out of the Dock, the app
    // switcher and Apple Events aimed at "Cyber CTF".
    #[cfg(target_os = "macos")]
    app.set_activation_policy(tauri::ActivationPolicy::Prohibited);
    let handle = app.handle().clone();
    tauri::async_runtime::spawn(async move {
        let code = run_logged(&handle, job).await;
        handle.exit(code);
    });
}

/// Stopped (the app's Stop sends SIGTERM to the worker's process group): the tools it runs
/// (status probes, and the vagrant/compose/terraform runs it streams) sit in groups of their own,
/// out of that signal's reach; end them too before going.
#[cfg(unix)]
fn end_tools_when_stopped(job_file: PathBuf) {
    tauri::async_runtime::spawn(async move {
        if let Ok(mut term) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            term.recv().await;
            crate::platform::exec::kill_live_tools();
            let _ = std::fs::remove_file(&job_file);
            std::process::exit(143);
        }
    });
}

/// Only the deploy ends the worker (`handle.exit(code)` carries its code). A quit from outside
/// (a script or tool quitting "Cyber CTF") reached it once and killed a deploy mid-way,
/// orphaning vagrant with no status written.
fn keep_running(event: &tauri::RunEvent) {
    if let tauri::RunEvent::ExitRequested { code: None, api, .. } = event {
        api.prevent_exit();
    }
}

/// macOS still lists the worker as a running "Cyber CTF": with the app itself closed, opening
/// Cyber CTF (Dock, Finder, `open`) reached the worker, which has no window, and nothing opened.
/// Hand such a request to the real app, and stay in the background (an AppleScript "activate"
/// had turned the worker into a foreground app).
#[cfg(target_os = "macos")]
fn hand_reopen_to_the_app(app: &AppHandle, event: &tauri::RunEvent) {
    if let tauri::RunEvent::Reopen { .. } = event {
        let _ = app.set_activation_policy(tauri::ActivationPolicy::Prohibited);
        open_main_app();
    }
}

fn read_job(path: &Path) -> Option<Job> {
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
    let Ok(f) = files::of(&crate::platform::AppDirs::from(app), &job.key()) else { return 2 };
    let sink = std::sync::Mutex::new(crate::platform::private_fs::append(&f.log).ok());
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
    // VM runs beside the lab's operations, without it. A lock that can't be taken fails the
    // job: running unlocked is what it is there to prevent.
    let held = match job.op {
        Op::AttackVm { .. } => Ok(None),
        _ => hold_lab(app, &job.lab_id).await.map(Some),
    };
    let result = match held {
        Ok(_lab) => execute(app, job, &log).await,
        Err(e) => Err(e),
    };
    conclude(&f, &result, launch, log)
}

/// The cross-process lock of lab `id`, waited for.
async fn hold_lab(app: &AppHandle, id: &str) -> crate::error::Result<super::lock::LabFileLock> {
    super::lock::acquire(&super::lock_path(app, id)?).await.map_err(super::lock::unavailable)
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
    // A refusal (the lab is still up elsewhere, an update of a running lab) changed nothing: the
    // launch's last verdict stays, or a lab still running on its host would read as failed.
    let refused = matches!(result, Err(crate::error::Error::Refused(_)));
    if launch && !refused {
        let _ = std::fs::write(&f.launch_status, &status);
    }
    let _ = std::fs::write(&f.status, status);
    // The job file carries the launch token: done with it.
    let _ = std::fs::remove_file(&f.job);
    // Only while it still names this worker: a newer one's pidfile stays.
    super::process::remove_pidfile_of(&f.pid, std::process::id());
    code
}

/// What the status file says for a result, and the exit code that goes with it.
fn status_line<T>(result: &crate::error::Result<T>) -> (String, i32) {
    match result {
        Ok(_) => ("ok".to_string(), 0),
        // As the webview gets it: the app reads its code back (`progress::verdict`).
        Err(e) => (format!("error: {}", serde_json::to_string(&e.to_wire()).unwrap_or_else(|_| e.to_string())), 1),
    }
}

#[cfg(test)]
mod tests {
    use super::{conclude, read_job, status_line};
    use crate::deploy_worker::files::Files;
    use crate::deploy_worker::progress::verdict;
    use crate::deploy_worker::{Job, Op};
    use crate::error::{Code, Error};

    #[test]
    fn the_status_line_reads_back_as_the_same_verdict() {
        assert_eq!(status_line(&Ok(())), ("ok".to_string(), 0));
        let (line, code) = status_line::<()>(&Err(Error::Invalid("Docker isn't running".into())));
        assert_eq!((line.as_str(), code), (r#"error: {"code":"other","message":"Docker isn't running"}"#, 1));
        assert_eq!(verdict(&line).unwrap_err().to_string(), "Docker isn't running");
        // A coded error comes back with its code, for the webview to say in its language.
        let (line, _) = status_line::<()>(&Err(Error::coded(Code::DockerNotRunning)));
        let back = verdict(&line).unwrap_err();
        assert_eq!(back.code(), Code::DockerNotRunning);
        assert_eq!(back.to_string(), Error::coded(Code::DockerNotRunning).to_string());
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
    fn a_job_file_with_a_byte_order_mark_or_trailing_text_is_refused() {
        // Hand-edited or half-overwritten job files: the worker exits rather than guessing.
        let dir = crate::deploy_worker::files::tests::temp_dir("job-odd");
        let path = dir.join("lab-1.json");
        let job = Job::on_lab("lab-1", Op::Resume { runtime: crate::runtime::Runtime::Vm });
        let json = serde_json::to_string(&job).unwrap();
        std::fs::write(&path, format!("\u{feff}{json}")).unwrap();
        assert!(read_job(&path).is_none(), "a BOM");
        std::fs::write(&path, format!("{json}\n{{}}")).unwrap();
        assert!(read_job(&path).is_none(), "two documents");
        std::fs::write(&path, format!("  {json}\n\n")).unwrap();
        assert_eq!(read_job(&path), Some(job), "whitespace around it is fine");
        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn an_attack_vm_job_never_takes_the_lab_lock() {
        // The attack VM runs beside the lab's own operation (a start may still be booting the
        // lab): its worker must not wait on, nor hold, the lab's lock.
        let t = crate::testing::test_app::TestApp::new();
        let _tools = crate::platform::exec::fake::Tools::rules(&[]);
        t.lab("ad", &[]);
        let lock = crate::deploy_worker::lock_path(t.handle(), "ad").unwrap();
        // Another process holds the lab (a handle of its own stands in for it).
        let other = std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(&lock).unwrap();
        other.lock().unwrap();
        let job = Job::on_lab("ad", Op::AttackVm { box_name: "kalilinux/rolling".into() });
        let run = tokio::time::timeout(std::time::Duration::from_secs(2), super::run_logged(t.handle(), job)).await;
        // Finished (refused: the lab's VMs aren't up) instead of waiting for the lock holder.
        assert_eq!(run.expect("did not wait for the lab lock"), 1);
        drop(other);
    }

    #[tokio::test]
    async fn a_job_whose_lab_lock_cant_be_taken_fails_instead_of_running_unlocked() {
        // Regression: the worker logged "no lab lock" and ran the job anyway.
        let t = crate::testing::test_app::TestApp::new();
        let tools = crate::platform::exec::fake::Tools::rules(&[]);
        t.lab("web", &[]);
        // A folder where the lock file goes: it can't be opened.
        std::fs::create_dir_all(crate::deploy_worker::lock_path(t.handle(), "web").unwrap()).unwrap();
        let job = Job::on_lab("web", Op::Resume { runtime: crate::runtime::Runtime::Docker });
        let f = crate::deploy_worker::files::of(&t.dirs(), &job.key()).unwrap();
        assert_eq!(super::run_logged(t.handle(), job).await, 1);
        let status = std::fs::read_to_string(&f.status).unwrap();
        assert!(verdict(&status).unwrap_err().to_string().starts_with("Couldn't lock the lab"), "{status}");
        assert!(tools.calls().is_empty(), "nothing ran: {:?}", tools.calls());
    }

    #[tokio::test]
    async fn a_run_logs_its_failure_and_leaves_its_verdict() {
        use crate::runtime::{Park, Runtime};
        let t = crate::testing::test_app::TestApp::new();
        let _tools = crate::platform::exec::fake::Tools::rules(&[]);
        for op in [
            Op::Resume { runtime: Runtime::Vm },
            Op::Park { runtime: Runtime::Docker, mode: Park::Pause },
            Op::Provision { runtime: Runtime::Vm, machine: None },
            Op::AttackVm { box_name: "kalilinux/rolling".into() },
            Op::Launch,
        ] {
            let job = Job::on_lab("gone", op);
            let f = crate::deploy_worker::files::of(&t.dirs(), &job.key()).unwrap();
            assert_eq!(super::run_logged(t.handle(), job).await, 1);
            let status = std::fs::read_to_string(&f.status).unwrap();
            assert!(status.starts_with("error: "), "{status}");
            assert!(std::fs::read_to_string(&f.log).unwrap().starts_with("✗ "));
            // Only this user reads the log, and the folder of jobs and logs.
            crate::platform::private_fs::tests::assert_private_with_folder(&f.log);
        }
    }

    #[test]
    fn concluding_writes_the_verdict_and_drops_the_pidfile() {
        let dir = crate::deploy_worker::files::tests::temp_dir("conclude");
        let f = Files::of(&dir, "lab-1");
        std::fs::write(&f.pid, std::process::id().to_string()).unwrap();
        std::fs::write(&f.job, "{}").unwrap();
        let lines = std::sync::Mutex::new(Vec::new());
        let code = conclude(&f, &Err::<(), _>(Error::Invalid("boom".into())), true, |l| lines.lock().unwrap().push(l));
        assert_eq!(code, 1);
        assert_eq!(lines.lock().unwrap().as_slice(), ["✗ boom"]);
        let failed = r#"error: {"code":"other","message":"boom"}"#;
        assert_eq!(std::fs::read_to_string(&f.status).unwrap(), failed);
        assert_eq!(std::fs::read_to_string(&f.launch_status).unwrap(), failed);
        assert!(!f.pid.exists());
        assert!(!f.job.exists(), "the job file (it carries the launch token) is gone once it ends");
        assert_eq!(conclude(&f, &Ok(()), true, |_| panic!("no log on success")), 0);
        assert_eq!(std::fs::read_to_string(&f.status).unwrap(), "ok");
        assert_eq!(std::fs::read_to_string(&f.launch_status).unwrap(), "ok");
        // Another operation leaves the launch's verdict alone.
        conclude(&f, &Err::<(), _>(Error::Invalid("park failed".into())), false, |_| {});
        assert_eq!(verdict(&std::fs::read_to_string(&f.status).unwrap()).unwrap_err().to_string(), "park failed");
        assert_eq!(std::fs::read_to_string(&f.launch_status).unwrap(), "ok");
        // So does a launch refused before it did anything (the lab is still up on its host).
        conclude(&f, &Err::<(), _>(Error::Refused("still running on AWS".into())), true, |_| {});
        assert_eq!(verdict(&std::fs::read_to_string(&f.status).unwrap()).unwrap_err().to_string(), "still running on AWS");
        assert_eq!(std::fs::read_to_string(&f.launch_status).unwrap(), "ok");
        std::fs::remove_dir_all(dir).ok();
    }
}
