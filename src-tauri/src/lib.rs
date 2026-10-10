mod account;
mod cloud;
mod config;
mod deploy_worker;
mod error;
mod exec;
mod labs;
mod lan_relay;
mod machine;
mod menu;
mod operations;
mod platform;
#[cfg(test)]
mod proptest_support;
mod provisioning;
mod quit;
mod runtime;
mod shared_folder;
mod terminal;
mod window;

use std::path::PathBuf;
use std::time::Duration;

use tauri::{App, AppHandle, Manager, RunEvent, WindowEvent};

/// How often expired cloud labs are looked for and torn down.
const REAP_EVERY: Duration = Duration::from_secs(300);

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
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
    tauri::Builder::default()
        // First: a cyberctf:// link opened while the app runs goes to that window
        // (Windows/Linux would otherwise start a second instance).
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            // The windows may be hidden (finishing in the background) or minimized: bring the
            // main one back, and keep the app open instead of exiting once its deploys end.
            quit::stop_lingering();
            window::reveal_main(app);
        }))
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
            machine::selftest::machine_selftest,
            machine::selftest::machine_selftest_prefetch,
            machine::workloads::machine_workloads,
            machine::workloads::machine_workload_stop,
            machine::storage::machine_storage,
            machine::storage::machine_storage_clean,
            account::auth::auth_login,
            account::auth::auth_status,
            account::auth::auth_logout,
            account::api::api_query,
            account::agent::agent_info,
            account::agent::confirm_launch,
            platform::install::install_dependency,
            platform::uninstall::installed_tools,
            platform::uninstall::uninstall_dependency,
            cloud::cloud_login,
            cloud::aws_cli_identity,
            cloud::aws_profiles,
            cloud::aws_login,
            cloud::aws_month_to_date_cost,
            cloud::azure_subscriptions,
            cloud::gcp_account,
            cloud::gcp_billing_accounts,
            cloud::gcp_organizations,
            cloud::oci_config,
            provisioning::provisioning_images,
            provisioning::provisioning_pull,
            platform::install::install_vagrant_plugin,
            labs::lab_launch,
            runtime::lab_start,
            runtime::lab_stop,
            runtime::lab_park,
            runtime::lab_resume,
            runtime::lab_provision,
            runtime::lab_status,
            runtime::running_labs,
            runtime::lab_check,
            runtime::lab_tools,
            runtime::exegol_status,
            runtime::exegol_start,
            runtime::exegol_stop,
            runtime::exegol_shell,
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
            runtime::server::server_list,
            runtime::server::server_save,
            runtime::server::server_remove,
            runtime::server::server_set_default,
            runtime::server::server_test,
            runtime::server::server_public_key,
            runtime::server::server_running_labs,
            runtime::server::server_capacity,
            runtime::server_selftest::server_selftest,
            runtime::server::server_open_setup,
            operations::deploy_in_progress,
            operations::deploying_labs,
            operations::stopping_labs,
            restart_app,
            operations::parking_labs,
            operations::active_operations,
            operations::lab_deploy_log,
            quit::linger_quit,
            quit::force_quit,
            window::open_settings,
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
    shared_folder::init(app.handle());
    // Linux and Windows dev builds: register cyberctf:// at runtime (installers do it otherwise).
    #[cfg(any(target_os = "linux", all(debug_assertions, windows)))]
    {
        use tauri_plugin_deep_link::DeepLinkExt;
        let _ = app.deep_link().register_all();
    }
    if cfg!(debug_assertions) {
        app.handle().plugin(tauri_plugin_log::Builder::default().level(log::LevelFilter::Info).build())?;
    }
    // Bring your own compute: register this machine and run the player's labs on it.
    account::agent::spawn(app.handle().clone());
    spawn_reaper(app.handle().clone());
    Ok(())
}

/// Timed teardown of expired cloud labs: once on startup, then every 5 minutes, so a forgotten
/// (or app-was-closed) lab stops billing instead of lingering.
fn spawn_reaper(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            runtime::reap_expired_labs(&app).await;
            tokio::time::sleep(REAP_EVERY).await;
        }
    });
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
