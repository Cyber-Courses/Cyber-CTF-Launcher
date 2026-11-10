//! The worker process itself: a headless Tauri app that runs one job and exits.

use std::io::Write as _;
use std::path::Path;
#[cfg(unix)]
use std::path::PathBuf;

use crate::AppHandle;
use tauri::Manager as _;

use super::files::status_line;
use super::{Job, Op, execute, files};
use crate::platform::unwind::{CatchUnwind, panic_message};
use crate::runtime::deploy_log::FAILED_MARK;
use crate::runtime::lab_file_lock;

/// The worker's entry point, in place of the app: a headless Tauri app on the same data dir
/// (so lab, key and server paths resolve the same), the deploy, a status file, then exit.
///
/// # Errors
///
/// When Tauri can't build the worker's app.
pub fn worker_main(job_path: &Path, mut context: tauri::Context<crate::AppRuntime>) -> tauri::Result<()> {
    // Windows starts the worker (a process group of its own) with Ctrl+C ignored, which its tools
    // would inherit: a stop's Ctrl+C never reached Terraform.
    #[cfg(windows)]
    crate::platform::console::let_tools_take_ctrl_c();
    let Some(job) = read_job(job_path) else {
        fail_before_the_job(job_path, &format!("its job file {} can't be read", job_path.display()));
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
            let task = start(app, job);
            #[cfg(unix)]
            end_tools_when_stopped(job_file, task);
            #[cfg(not(unix))]
            drop(task);
            Ok(())
        })
        .build(context)
        .inspect_err(|e| fail_before_the_job(job_path, &format!("Tauri couldn't build it: {e}")))?;
    #[cfg(target_os = "macos")]
    app.run(|app, event| {
        keep_running(&event);
        hand_reopen_to_the_app(app, &event);
    });
    #[cfg(not(target_os = "macos"))]
    app.run(|_, event| keep_running(&event));
    Ok(())
}

/// The worker's start: its log and data folder, then the job, and an exit with its code. Returns
/// the job's task, which a stop ends.
fn start(app: &mut tauri::App<crate::AppRuntime>, job: Job) -> tauri::async_runtime::JoinHandle<()> {
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
    })
}

/// How long a stopped worker waits for the runner's tools it cut short to stop on their own
/// before it ends them: within the 20 s the app's Stop gives a worker before it kills it
/// (`process::kill_and_wait`), which would leave them running.
#[cfg(unix)]
const TOOLS_GRACE: std::time::Duration = std::time::Duration::from_secs(15);

/// Stopped (the app's Stop sends SIGTERM to the worker's process group): the tools it runs
/// (status probes, and the vagrant/compose/terraform runs it streams) sit in groups of their own,
/// out of that signal's reach; end them too before going. The launcher's own are ended gently
/// first; then the job's task is ended, which drops an Isothrum runner's operation: the runner
/// asks the tools it started to stop (SIGINT), and they are waited for ([`TOOLS_GRACE`]), what
/// is left ended, before the worker exits.
#[cfg(unix)]
fn end_tools_when_stopped(job_file: PathBuf, task: tauri::async_runtime::JoinHandle<()>) {
    tauri::async_runtime::spawn(async move {
        if let Ok(mut term) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            term.recv().await;
            crate::platform::exec::kill_live_tools();
            task.abort();
            // Cancelled, as asked: the job's own outcome is moot.
            drop(task.await);
            // The runner's tools it cut short are asked to stop (Terraform saves its state):
            // waited for, within the app's patience with a stopped worker, so none outlives it.
            crate::runtime::isothrum::stop_cut_short_tools(TOOLS_GRACE).await;
            files::remove_job(&job_file);
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
        if let Err(e) = app.set_activation_policy(tauri::ActivationPolicy::Prohibited) {
            log::warn!("couldn't keep the worker in the background: {e}");
        }
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
    let opened = match bundle.filter(|b| b.extension().is_some_and(|e| e == "app")) {
        Some(b) => std::process::Command::new("/usr/bin/open").arg("-n").arg(&b).spawn(),
        // Not in a bundle (a dev build): run the binary without the worker arguments.
        None => std::env::current_exe().and_then(|exe| std::process::Command::new(exe).spawn()),
    };
    if let Err(e) = opened {
        log::warn!("couldn't open the app for a reopen the worker got: {e}");
    }
}

/// Runs the job, logging to its file (also this process's stdout/stderr, for panics), and leaves
/// the verdict in the status file. Returns the process's exit code.
async fn run_logged(app: &AppHandle, job: Job) -> i32 {
    let f = match files::of(&crate::platform::AppDirs::from(app), &job.key()) {
        Ok(f) => f,
        Err(e) => {
            log::error!("deploy worker for {}: no files to run the job in: {e}", job.key());
            return 2;
        }
    };
    let sink = std::sync::Mutex::new(open_log(&f.log));
    let log = |line: String| {
        if let Ok(mut guard) = sink.lock()
            && let Some(file) = guard.as_mut()
        {
            let _ = writeln!(file, "{line}");
        }
    };
    let launch = brings_up(&job.op);
    // The lab is this worker's for the whole job (download and install included): the app's
    // own operations on it (a stop, the expiry reaper) wait or step aside meanwhile. A lock that
    // can't be taken fails the job: running unlocked is what it is there to prevent.
    let held = hold_lab(app, &job.lab_id).await.map(Some);
    let result = match held {
        // Boxed: every operation's future is in it, too large to keep on the caller's stack. A
        // panic in it is its verdict: uncaught, it ended the job's task alone, and the worker
        // stayed up with no status written, the lab "starting" for good.
        Ok(_lab) => CatchUnwind(Box::pin(execute(app, job, &log))).await.unwrap_or_else(|panic| Err(crashed(&*panic))),
        Err(e) => Err(e),
    };
    conclude(&f, &result, launch, log)
}

/// The verdict of a job that panicked (the panic hook logged where, with a backtrace, in the
/// worker's own log file).
fn crashed(panic: &(dyn std::any::Any + Send)) -> crate::error::Error {
    crate::error::Error::Invalid(format!("the deploy worker crashed: {}", panic_message(panic)))
}

/// A worker that can't run its job at all (`why`): said on its stderr (its job's log) and as its
/// verdict in the status file beside the job file at `job_path` (`<key>.json`, `<key>.status`), so
/// the app says why rather than "stopped without a result". Before the log file is set up: this
/// is all the trace there is.
fn fail_before_the_job(job_path: &Path, why: &str) {
    let error = crate::error::Error::Invalid(format!("the deploy worker couldn't start: {why}"));
    eprintln!("{FAILED_MARK} {error}");
    let (status, _) = status_line::<()>(&Err(error));
    if let Err(e) = std::fs::write(job_path.with_extension("status"), status) {
        eprintln!("deploy worker: couldn't write its status beside {}: {e}", job_path.display());
    }
}

/// The job's log file, opened for appending; none when it can't be (the job still runs, and the
/// app's log says why its own is empty).
fn open_log(path: &std::path::Path) -> Option<std::fs::File> {
    crate::platform::private_fs::append(path).inspect_err(|e| log::warn!("the deploy log {} can't be written: {e}", path.display())).ok()
}

/// Whether `op` brings the lab up, its verdict then saying whether it came up
/// (`files::Files::launch_status`): a launch, from the catalogue or a local folder, and a resume.
fn brings_up(op: &Op) -> bool {
    matches!(op, Op::Launch | Op::LaunchLocal { .. } | Op::Resume { .. })
}

/// The cross-process lock of lab `id`, waited for.
async fn hold_lab(app: &AppHandle, id: &str) -> crate::error::Result<lab_file_lock::LabFileLock> {
    lab_file_lock::acquire(&lab_file_lock::path(&crate::platform::AppDirs::from(app), id)?).await.map_err(lab_file_lock::unavailable)
}

/// The end of a run: the failure in the log, the verdict in the status file (and, for a
/// `launch` or a resume, in the launch status too), the pidfile gone. Returns the process's exit code.
fn conclude<T>(f: &files::Files, result: &crate::error::Result<T>, launch: bool, log: impl Fn(String)) -> i32 {
    // The log file says how it ended too, so a page that re-attaches to it after a reload or a
    // relaunch (it has no live verdict) still sees the run failed.
    if let Err(e) = result {
        log(format!("{FAILED_MARK} {e}"));
    }
    let (status, code) = status_line(result);
    // A refusal (the lab is still up elsewhere, an update of a running lab) changed nothing: the
    // launch's last verdict stays, or a lab still running on its host would read as failed.
    let refused = result.as_ref().is_err_and(crate::error::Error::is_refusal);
    if launch
        && !refused
        && let Err(e) = std::fs::write(&f.launch_status, &status)
    {
        log::error!("couldn't write the launch status {}: {e}", f.launch_status.display());
    }
    // The app reads the verdict here alone: a run whose status isn't written reads as still going.
    if let Err(e) = std::fs::write(&f.status, status) {
        log::error!("couldn't write the deploy status {}: {e}", f.status.display());
    }
    // The job file carries the launch token: done with it.
    files::remove_job(&f.job);
    // Only while it still names this worker: a newer one's pidfile stays.
    super::process::remove_pidfile_of(&f.pid, std::process::id());
    code
}

#[cfg(test)]
mod tests {
    use super::{brings_up, conclude, read_job, status_line};
    use crate::app::deploy_worker::files::{Files, verdict};
    use crate::app::deploy_worker::{Job, Op};
    use crate::error::{Code, Error};

    #[test]
    fn a_deploy_log_that_cant_be_opened_is_said_in_the_apps_log() {
        // Regression: the job ran without its log, and nothing said why.
        let dir = std::env::temp_dir().join(format!("cyberctf-log-dir-{}", rand::random::<u32>()));
        std::fs::create_dir_all(&dir).unwrap();
        crate::testing::logs::take_warnings();
        assert!(super::open_log(&dir).is_none(), "a folder isn't a log file");
        let logged = crate::testing::logs::take_warnings();
        assert!(logged.iter().any(|w| w.starts_with("the deploy log ") && w.contains("can't be written")), "{logged:?}");
        std::fs::remove_dir_all(dir).unwrap();
    }

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
        let dir = crate::testing::temp_dir("job");
        let path = dir.join("lab-1.json");
        assert!(read_job(&path).is_none());
        let job = Job::on_lab("lab-1", Op::Provision { runtime: crate::runtime::Runtime::Vm, machine: Some("dc01".into()) });
        std::fs::write(&path, serde_json::to_vec(&job).unwrap()).unwrap();
        assert_eq!(read_job(&path), Some(job));
        std::fs::write(&path, "{").unwrap();
        assert!(read_job(&path).is_none());
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_job_file_with_a_byte_order_mark_or_trailing_text_is_refused() {
        // Hand-edited or half-overwritten job files: the worker exits rather than guessing.
        let dir = crate::testing::temp_dir("job-odd");
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
    async fn a_job_whose_lab_lock_cant_be_taken_fails_instead_of_running_unlocked() {
        // Regression: the worker logged "no lab lock" and ran the job anyway.
        let t = crate::testing::test_app::TestApp::new();
        let tools = crate::platform::exec::fake::Tools::rules(&[]);
        t.lab("web", &[]);
        // A folder where the lock file goes: it can't be opened.
        std::fs::create_dir_all(crate::runtime::lab_file_lock::path(&t.dirs(), "web").unwrap()).unwrap();
        let job = Job::on_lab("web", Op::Resume { runtime: crate::runtime::Runtime::Docker });
        let f = crate::app::deploy_worker::files::of(&t.dirs(), &job.key()).unwrap();
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
            Op::Launch,
        ] {
            let job = Job::on_lab("gone", op);
            let f = crate::app::deploy_worker::files::of(&t.dirs(), &job.key()).unwrap();
            assert_eq!(super::run_logged(t.handle(), job).await, 1);
            let status = std::fs::read_to_string(&f.status).unwrap();
            assert!(status.starts_with("error: "), "{status}");
            assert!(std::fs::read_to_string(&f.log).unwrap().starts_with("✗ "));
            // Only this user reads the log, and the folder of jobs and logs.
            crate::platform::private_fs::tests::assert_private_with_folder(&f.log);
        }
    }

    #[tokio::test]
    async fn a_job_that_panics_ends_with_its_verdict_instead_of_hanging() {
        // Regression: a panic in the job ended its task alone. No verdict was written and the
        // worker stayed up, so the app showed the lab starting for as long as it ran.
        let t = crate::testing::test_app::TestApp::new();
        let _tools = crate::platform::exec::fake::Tools::new(|program, _| panic!("{program} blew up"));
        let commit = "a".repeat(40);
        let spec = serde_json::json!({ "labId": "web", "runtime": "DOCKER", "repository": "CyberCTF/web", "commit": commit, "env": [] });
        let launch = crate::app::deploy_worker::LaunchJob { launch: crate::labs::parse_launch(spec).unwrap(), place: crate::runtime::Placement::default() };
        let job = Job { launch: Some(launch), ..Job::on_lab("web", Op::Launch) };
        let f = crate::app::deploy_worker::files::of(&t.dirs(), &job.key()).unwrap();
        assert_eq!(super::run_logged(t.handle(), job).await, 1);
        let said = verdict(&std::fs::read_to_string(&f.status).unwrap()).unwrap_err().to_string();
        assert_eq!(said, "the deploy worker crashed: docker blew up");
        assert!(verdict(&std::fs::read_to_string(&f.launch_status).unwrap()).is_err(), "the launch failed");
        assert!(std::fs::read_to_string(&f.log).unwrap().contains("✗ the deploy worker crashed: docker blew up"));
    }

    #[test]
    fn a_worker_that_cant_run_its_job_leaves_why_as_its_verdict() {
        // Regression: an unreadable job file (or a worker Tauri couldn't build) exited with only
        // a line on stderr, and the app said "stopped without a result".
        let dir = crate::testing::temp_dir("fail-early");
        let f = Files::of(&dir, "lab-1");
        super::fail_before_the_job(&f.job, "its job file can't be read");
        let said = verdict(&std::fs::read_to_string(&f.status).unwrap()).unwrap_err().to_string();
        assert_eq!(said, "the deploy worker couldn't start: its job file can't be read");
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn concluding_writes_the_verdict_and_drops_the_pidfile() {
        let dir = crate::testing::temp_dir("conclude");
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
        // A launch refused for something to put right first (a missing value, an expired cloud
        // sign-in) failed: its verdict says so.
        conclude(&f, &Err::<(), _>(Error::RefusedBeforeActing("sign in again".into())), true, |_| {});
        assert_eq!(verdict(&std::fs::read_to_string(&f.launch_status).unwrap()).unwrap_err().to_string(), "sign in again");
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_launch_and_a_resume_say_whether_the_lab_came_up_a_pause_or_a_provisioning_run_doesnt() {
        use crate::runtime::{Park, Runtime};
        assert!(brings_up(&Op::Launch));
        assert!(brings_up(&Op::LaunchLocal { src: "/labs/web".into() }));
        assert!(brings_up(&Op::Resume { runtime: Runtime::Vm }));
        assert!(!brings_up(&Op::Park { runtime: Runtime::Docker, mode: Park::Pause }));
        assert!(!brings_up(&Op::Provision { runtime: Runtime::Vm, machine: None }));
    }
}
