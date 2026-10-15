//! The app shell: starting the launcher (or a deploy worker), its plugins, windows, menu and
//! quitting, and the log file.

pub mod logging;
pub mod menu;
pub mod quit;
pub mod setup_window;
pub mod window;

use std::path::PathBuf;
use std::time::Duration;

use tauri::{Manager, RunEvent, WindowEvent};

use crate::platform::{exec, private_fs};
use crate::{AppHandle, AppRuntime, account, commands, deploy_worker, platform, runtime, shared_folder, terminal};

type App = tauri::App<AppRuntime>;

impl From<&AppHandle> for platform::AppDirs {
    /// The app's folders, as Tauri resolves them for this app.
    fn from(app: &AppHandle) -> platform::AppDirs {
        let paths = app.path();
        platform::AppDirs::new(paths.app_data_dir().ok(), paths.app_cache_dir().ok())
    }
}

/// Opens folder `path` in the system's file manager (through the opener plugin).
pub fn open_folder(path: &std::path::Path) -> crate::error::Result<()> {
    tauri_plugin_opener::open_path(path, None::<&str>).map_err(|e| crate::error::Error::Invalid(format!("couldn't open {}: {e}", path.display())))
}

/// How often expired cloud labs are looked for and torn down.
const REAP_EVERY: Duration = Duration::from_secs(300);

/// Runs the launcher, as the app or as a deploy worker (see `crate::run`).
///
/// # Errors
///
/// When Tauri can't build the app (its embedded configuration is broken).
pub fn run() -> tauri::Result<()> {
    // First: a panic anywhere after this lands in the log file, not in a stderr nobody sees.
    logging::install_panic_hook();
    // `cyberctf-desktop interrupt --pid <pid>` (Windows): Ctrl+C a deploy's Terraform so it stops
    // cleanly, from a throwaway process (see platform::console). Nothing else of the app starts.
    #[cfg(windows)]
    if let Some(pid) = platform::console::interrupt_pid(std::env::args()) {
        std::process::exit(if platform::console::send_ctrl_c(pid) { 0 } else { 1 });
    }
    runtime::server::mark_started();
    // Before anything else: GUI launches don't get the shell PATH (docker, vagrant, ovftool).
    platform::env_path::augment();
    // One context (the embedded config and assets) for whichever role this process plays.
    let context = tauri::generate_context!();
    // `cyberctf-desktop deploy --job <file>`: this process is a deploy worker, not the app. It
    // runs one lab's deploy headless and exits; the app that started it only follows its log, so
    // quitting the app never cuts a deploy short.
    if let Some(job) = worker_job(std::env::args()) {
        return deploy_worker::worker_main(&job, context);
    }
    let app = commands::register(app_builder()).build(context).inspect_err(|e| log::error!("couldn't build Cyber CTF: {e}"))?;
    app.run(|app, event| on_run_event(app, &event));
    Ok(())
}

/// The app: its plugins, window and menu events, and setup.
fn app_builder() -> tauri::Builder<AppRuntime> {
    tauri::Builder::<AppRuntime>::new()
        // First: a cyberctf:// link opened while the app runs goes to that window
        // (Windows/Linux would otherwise start a second instance).
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            // The windows may be hidden (finishing in the background) or minimized: bring the
            // main one back, and keep the app open instead of exiting once its deploys end.
            quit::stop_lingering();
            window::reveal_main(app);
        }))
        .plugin(logging::plugin(logging::APP_LOG))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_notification::init())
        // Quitting mid-deploy can leave resources running (a cloud apply keeps billing). Hold the
        // window open and let the frontend confirm; the red-button close is handled here, Cmd+Q /
        // app exit in `on_run_event`.
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event
                && quit::intercept(window.app_handle())
            {
                api.prevent_close();
            }
        })
        .menu(menu::build)
        .on_menu_event(|app, event| menu::on_event(app, &event))
        .setup(|app| {
            setup(app);
            Ok(())
        })
}

/// The job file when this process was started as a deploy worker (`<exe> deploy --job <file>`).
fn worker_job(args: impl IntoIterator<Item = String>) -> Option<PathBuf> {
    let args: Vec<String> = args.into_iter().collect();
    match args.as_slice() {
        [_, deploy, flag, job, ..] if deploy == "deploy" && flag == "--job" => Some(PathBuf::from(job)),
        _ => None,
    }
}

fn setup(app: &mut App) {
    logging::log_startup(app.handle(), "app");
    // Before anything reads its settings: what a virtualized Store build saved.
    #[cfg(windows)]
    {
        let paths = app.path();
        platform::msix_data::migrate(&app.config().identifier, paths.app_local_data_dir().ok().as_deref(), paths.app_data_dir().ok().as_deref());
    }
    // The window a UAC prompt (Vagrant on Hyper-V) is raised for, here and in deploy workers.
    #[cfg(windows)]
    if let Some(w) = app.get_webview_window("main")
        && let Ok(hwnd) = w.hwnd()
    {
        exec::set_owner_window(hwnd.0 as isize);
    }
    // The app's data (deploy jobs, Terraform state, keys) is this user's alone.
    if let Ok(dir) = app.path().app_data_dir() {
        let _ = private_fs::dir(&dir);
        shared_folder::init(&dir);
    }
    // Linux and Windows dev builds: register cyberctf:// at runtime (installers do it otherwise).
    #[cfg(any(target_os = "linux", all(debug_assertions, windows)))]
    {
        use tauri_plugin_deep_link::DeepLinkExt;
        let _ = app.deep_link().register_all();
    }
    // Sign-in coming back as a cyberctf://auth/callback link goes to the sign-in waiting for it
    // (lab links stay the frontend's), and the app comes forward from the browser.
    {
        use tauri_plugin_deep_link::DeepLinkExt;
        let handle = app.handle().clone();
        app.deep_link().on_open_url(move |event| {
            if event.urls().iter().any(account::auth::deliver_link) {
                window::reveal_main(&handle);
            }
        });
    }
    // Bring your own compute: register this machine and run the player's labs on it.
    account::agent::spawn(app.handle().clone());
    spawn_reaper(app.handle().clone());
}

/// Timed teardown of expired cloud labs: once on startup, then every 5 minutes, so a forgotten
/// (or app-was-closed) lab stops billing instead of lingering.
fn spawn_reaper(app: AppHandle) {
    tauri::async_runtime::spawn(every_pass(REAP_EVERY, move || {
        let app = app.clone();
        async move { Box::pin(runtime::reap_expired_labs(&app)).await }
    }));
}

/// Runs `pass` now, then again `every` after each one ends, each pass on a task of its own: a
/// panic in one pass (the panic hook logs it) ends that pass, not the loop. A bare loop ended
/// for good on the first panic, and expired cloud labs then kept billing until a restart.
async fn every_pass<F, Fut>(every: Duration, mut pass: F)
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = ()> + Send + 'static,
{
    loop {
        if let Err(e) = tauri::async_runtime::spawn(pass()).await {
            log::error!("a background pass ended early: {e}");
        }
        tokio::time::sleep(every).await;
    }
}

fn on_run_event(app: &AppHandle, event: &RunEvent) {
    // Cmd+Q, the Quit menu item and a system shutdown come through here, not the window close
    // event. Intercept the same way, so a deploy in progress isn't cut off.
    if let RunEvent::ExitRequested { api, code, .. } = event
        && code.is_none()
        && quit::intercept(app)
    {
        api.prevent_exit();
    }
    if let RunEvent::Exit = event {
        exec::kill_live_tools();
        terminal::close_all();
    }
}

#[cfg(test)]
mod tests {
    use super::worker_job;

    #[tokio::test]
    async fn a_panicking_pass_never_ends_the_loop() {
        // Regression: the reaper was a bare loop, so one panic in a pass stopped cloud auto-stop
        // for the rest of the session.
        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering};
        let passes = Arc::new(AtomicUsize::new(0));
        let counted = passes.clone();
        let looping = tokio::spawn(super::every_pass(std::time::Duration::from_millis(1), move || {
            let n = counted.fetch_add(1, Ordering::SeqCst);
            async move { assert!(n > 0, "the first pass panics") }
        }));
        let waited = tokio::time::timeout(std::time::Duration::from_secs(5), async {
            while passes.load(Ordering::SeqCst) < 3 {
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
        })
        .await;
        looping.abort();
        assert!(waited.is_ok(), "passes ran after the one that panicked");
    }

    #[test]
    fn only_deploy_job_arguments_make_a_worker() {
        let args = |a: &[&str]| a.iter().map(ToString::to_string).collect::<Vec<_>>();
        assert_eq!(worker_job(args(&["app", "deploy", "--job", "/d/lab.json"])), Some("/d/lab.json".into()));
        assert_eq!(worker_job(args(&["app", "deploy", "--job", "/d/lab.json", "extra"])), Some("/d/lab.json".into()));
        assert_eq!(worker_job(args(&["app", "deploy", "--job"])), None);
        assert_eq!(worker_job(args(&["app", "deploy", "--other", "x"])), None);
        assert_eq!(worker_job(args(&["app"])), None);
    }
}
