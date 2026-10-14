mod account;
mod atomic_file;
mod cloud;
mod config;
mod deploy_worker;
mod error;
mod exec;
mod http;
mod labs;
mod lan_relay;
mod logging;
mod machine;
mod menu;
mod msix_data;
mod operations;
mod platform;
mod private_fs;
#[cfg(test)]
mod proptest_support;
mod quit;
mod runtime;
mod shared_folder;
#[cfg(test)]
mod source_scan;
mod terminal;
#[cfg(test)]
mod test_app;
#[cfg(test)]
mod ts_bindings;
mod window;

use std::path::PathBuf;
use std::time::Duration;

use tauri::{Manager, RunEvent, WindowEvent};

/// The Tauri runtime the app runs on: the real webview, or Tauri's mock in unit tests, so code
/// that takes an app handle (paths under the app data folder, the commands) runs there too.
#[cfg(not(test))]
pub(crate) type AppRuntime = tauri::Wry;
#[cfg(test)]
pub(crate) type AppRuntime = tauri::test::MockRuntime;
pub(crate) type AppHandle = tauri::AppHandle<AppRuntime>;
pub(crate) type WebviewWindow = tauri::WebviewWindow<AppRuntime>;
type App = tauri::App<AppRuntime>;

/// How often expired cloud labs are looked for and torn down.
const REAP_EVERY: Duration = Duration::from_secs(300);

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
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
        deploy_worker::worker_main(job, context);
        return;
    }
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
        .on_menu_event(menu::on_event)
        .setup(setup)
        .invoke_handler(tauri::generate_handler![
            machine::system::system_check,
            machine::images::image_download_size,
            machine::system::docker::docker_use_engine,
            machine::system::docker::docker_start_engine,
            machine::system::metrics::machine_metrics,
            machine::system::setup_window::machine_open_setup,
            machine::system::wsl::machine_restart,
            machine::selftest::machine_selftest,
            machine::selftest::machine_selftest_prefetch,
            machine::workloads::machine_workloads,
            machine::workloads::machine_workload_stop,
            machine::storage::machine_storage,
            machine::storage::machine_storage_clean,
            account::auth::auth_login,
            account::auth::auth_login_cancel,
            account::auth::auth_status,
            account::auth::auth_logout,
            account::api::api_query,
            account::agent::agent_info,
            account::agent::confirm_launch,
            platform::install::install_dependency,
            platform::uninstall::installed_tools,
            platform::uninstall::uninstall_dependency,
            cloud::cloud_login,
            cloud::cloud_login_cancel,
            cloud::aws_cli_identity,
            cloud::aws_profiles,
            cloud::aws_login,
            cloud::aws_month_to_date_cost,
            cloud::azure_subscriptions,
            cloud::gcp_account,
            cloud::gcp_billing_accounts,
            cloud::gcp_organizations,
            runtime::providers::hyperv_join_admins,
            cloud::oci_config,
            platform::install::install_vagrant_plugin,
            labs::lab_launch,
            runtime::lab_stop,
            runtime::lab_park,
            runtime::lab_resume,
            runtime::lab_provision,
            runtime::lab_status,
            runtime::lab_cloud_preview,
            runtime::running_labs,
            runtime::lab_check,
            runtime::lab_tools,
            runtime::exegol_status,
            runtime::exegol_start,
            runtime::exegol_stop,
            runtime::attack_vm_status,
            runtime::attack_vm_start,
            runtime::attack_vm_stop,
            runtime::attack_vm_shell,
            runtime::lab_attack_shell,
            terminal::terminal_open,
            terminal::terminal_write,
            terminal::terminal_resize,
            terminal::terminal_close,
            terminal::terminal_window,
            shared_folder::shared_folder_get,
            shared_folder::shared_folder_set,
            shared_folder::shared_folder_open,
            runtime::lab::registry::isoloom_registry_get,
            runtime::lab::registry::isoloom_registry_set,
            runtime::server::server_list,
            runtime::server::server_save,
            runtime::server::server_remove,
            runtime::server::server_set_default,
            runtime::server::server_test,
            runtime::server::server_public_key,
            runtime::server::server_running_labs,
            runtime::server::server_capacity,
            runtime::server::server_month_to_date_cost,
            runtime::server_selftest::server_selftest,
            runtime::server::server_open_setup,
            operations::deploying_labs,
            restart_app,
            operations::active_operations,
            operations::lab_deploy_log,
            quit::linger_quit,
            quit::force_quit,
            window::open_settings,
            logging::open_log_folder,
        ])
        .build(context)
        .expect("error while building Cyber CTF")
        .run(on_run_event);
}

/// The job file when this process was started as a deploy worker (`<exe> deploy --job <file>`).
fn worker_job(args: impl IntoIterator<Item = String>) -> Option<PathBuf> {
    let args: Vec<String> = args.into_iter().collect();
    match args.as_slice() {
        [_, deploy, flag, job, ..] if deploy == "deploy" && flag == "--job" => Some(PathBuf::from(job)),
        _ => None,
    }
}

fn setup(app: &mut App) -> std::result::Result<(), Box<dyn std::error::Error>> {
    logging::log_startup(app.handle(), "app");
    // Before anything reads its settings: what a virtualized Store build saved.
    #[cfg(windows)]
    msix_data::migrate(app.handle());
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
    }
    shared_folder::init(app.handle());
    runtime::lab::registry::init(app.handle());
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
    Ok(())
}

/// Timed teardown of expired cloud labs: once on startup, then every 5 minutes, so a forgotten
/// (or app-was-closed) lab stops billing instead of lingering.
fn spawn_reaper(app: AppHandle) {
    tauri::async_runtime::spawn(every_pass(REAP_EVERY, move || {
        let app = app.clone();
        async move { runtime::reap_expired_labs(&app).await }
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

fn on_run_event(app: &AppHandle, event: RunEvent) {
    // Cmd+Q, the Quit menu item and a system shutdown come through here, not the window close
    // event. Intercept the same way, so a deploy in progress isn't cut off.
    if let RunEvent::ExitRequested { api, code, .. } = &event
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

/// Relaunches the app, so an installed update applies without the player quitting by hand.
#[tauri::command]
fn restart_app(app: AppHandle) {
    app.restart();
}

#[cfg(test)]
mod tests {
    use super::worker_job;
    use std::collections::BTreeSet;

    /// The quoted or path-final names between the line starting `start` and the next `end`.
    fn names_between(src: &str, start: &str, end: &str) -> BTreeSet<String> {
        let body = src.split_once(start).map(|(_, rest)| rest).unwrap_or_default();
        let body = body.split_once(end).map(|(list, _)| list).unwrap_or_default();
        body.split(',').map(|n| n.trim().trim_matches('"').rsplit("::").next().unwrap_or_default().to_string()).filter(|n| !n.is_empty()).collect()
    }

    /// build.rs lists the commands the app manifest makes permissions for (each window's
    /// capability is built on them): exactly the ones registered with `generate_handler!`.
    #[test]
    fn the_app_manifest_lists_every_registered_command() {
        let registered = names_between(include_str!("lib.rs"), "generate_handler![", "])");
        let manifest = names_between(include_str!("../build.rs"), "const COMMANDS: &[&str] = &[", "];");
        assert!(registered.len() > 50, "{registered:?}");
        assert_eq!(manifest, registered);
    }

    /// Commands no unit test can call, each with why. Everything else in build.rs's list is
    /// called by name from a test.
    const NOT_UNIT_TESTED: &[(&str, &str)] = &[
        ("auth_login", "opens the system browser and waits for cyber-auth's redirect; authorize_url and the callback are tested"),
        ("restart_app", "restarts this very process"),
        ("open_log_folder", "opens the system's file manager; log_dir is tested"),
    ];

    /// Whether `code` calls `name(…)` (not `other_name(…)`), or invokes it through Tauri's IPC
    /// (`cmd: "name"`).
    fn calls(code: &str, name: &str) -> bool {
        code.contains(&format!("cmd: \"{name}\""))
            || code.match_indices(name).any(|(at, _)| {
                let before = code[..at].chars().next_back().is_some_and(|c| c.is_alphanumeric() || c == '_');
                !before && code[at + name.len()..].trim_start().starts_with('(')
            })
    }

    /// A command no test calls is one whose argument checks, plumbing and errors nobody checks.
    #[test]
    fn every_command_is_called_by_a_test() {
        let commands = names_between(include_str!("../build.rs"), "const COMMANDS: &[&str] = &[", "];");
        let tests: String = crate::source_scan::crate_sources().into_iter().map(|s| s.tests + "\n").collect();
        let exempt = |c: &str| NOT_UNIT_TESTED.iter().any(|(n, _)| *n == c);
        let untested: Vec<&String> = commands.iter().filter(|c| !exempt(c) && !calls(&tests, c)).collect();
        assert!(untested.is_empty(), "commands no test calls (test them, or add them to NOT_UNIT_TESTED with why): {untested:?}");
        for (name, _) in NOT_UNIT_TESTED {
            assert!(commands.contains(*name), "{name} is no longer a command");
        }
    }

    #[test]
    fn a_call_is_told_apart_from_a_longer_name() {
        assert!(calls("let x = lab_status (app, id);", "lab_status"));
        assert!(!calls("server_lab_status(app)", "lab_status"));
        assert!(!calls("\"lab_status\",", "lab_status"));
        assert!(calls("InvokeRequest { cmd: \"lab_status\".into(), ..", "lab_status"));
    }

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
        let args = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(worker_job(args(&["app", "deploy", "--job", "/d/lab.json"])), Some("/d/lab.json".into()));
        assert_eq!(worker_job(args(&["app", "deploy", "--job", "/d/lab.json", "extra"])), Some("/d/lab.json".into()));
        assert_eq!(worker_job(args(&["app", "deploy", "--job"])), None);
        assert_eq!(worker_job(args(&["app", "deploy", "--other", "x"])), None);
        assert_eq!(worker_job(args(&["app"])), None);
    }
}
