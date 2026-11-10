//! The commands the webview invokes, registered with Tauri; each window may call only those its
//! capability grants (build.rs, capabilities/). Each is a thin adapter: it turns the webview's
//! arguments and channels into the domain's types and calls the domain's function. The lab and
//! server-host commands live here too (`labs`, `server`, `setup_window`), alongside the rest.
// Tauri builds a command's arguments from the webview's message and hands each over by value: an
// app handle or a window can't be borrowed, so a command that only reads one still takes it.
#![allow(clippy::needless_pass_by_value, reason = "Tauri hands a command its arguments by value")]

mod account;
pub(crate) mod app;
mod cloud;
pub(crate) mod labs;
mod machine;
pub(crate) mod server;
mod setup_window;
mod shared_folder;
mod terminal;

use tauri::ipc::Channel;

use crate::AppRuntime;

/// A line sink that sends each line on `logs` (a closed window drops them). An `Fn`, so it also
/// serves where a callback is shared or called through `&`.
fn lines(logs: Channel<String>) -> impl Fn(String) + Send + Sync + 'static {
    move |line| {
        let _ = logs.send(line);
    }
}

/// Registers every command the webview invokes.
#[allow(clippy::too_many_lines, reason = "one line per command: a list, not logic")]
pub fn register(builder: tauri::Builder<AppRuntime>) -> tauri::Builder<AppRuntime> {
    builder.invoke_handler(tauri::generate_handler![
        machine::system_check,
        machine::image_download_size,
        machine::docker_use_engine,
        machine::docker_start_engine,
        machine::machine_metrics,
        machine::machine_open_setup,
        machine::machine_restart,
        machine::machine_selftest,
        machine::machine_selftest_prefetch,
        machine::machine_workloads,
        machine::machine_workload_stop,
        machine::machine_storage,
        machine::machine_storage_clean,
        account::auth_login,
        account::auth_login_cancel,
        account::auth_login_device,
        account::auth_status,
        account::auth_logout,
        account::api_query,
        account::agent_info,
        account::confirm_launch,
        machine::install_dependency,
        machine::installed_tools,
        machine::uninstall_dependency,
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
        machine::hyperv_join_admins,
        cloud::oci_config,
        machine::install_vagrant_plugin,
        machine::ovftool_place,
        labs::lab_launch,
        labs::lab_launch_local,
        labs::lab_stop,
        labs::lab_park,
        labs::lab_resume,
        labs::lab_provision,
        labs::lab_status,
        labs::lab_cloud_preview,
        labs::lab_requirements,
        labs::lab_default_place,
        labs::lab_run_options,
        labs::running_labs,
        labs::lab_check,
        labs::lab_tools,
        labs::attack_box_status,
        labs::attack_box_attach,
        labs::attack_box_detach,
        labs::lab_attack_shell,
        terminal::terminal_open,
        terminal::terminal_write,
        terminal::terminal_resize,
        terminal::terminal_close,
        terminal::terminal_window,
        shared_folder::shared_folder_get,
        shared_folder::shared_folder_set,
        shared_folder::shared_folder_open,
        server::server_list,
        server::server_save,
        server::server_save_ssh_key,
        server::server_proxmox_placement,
        server::server_default_ssh_key,
        server::server_remove,
        server::server_set_default,
        server::server_test,
        server::server_public_key,
        server::server_running_labs,
        server::server_capacity,
        server::server_month_to_date_cost,
        server::server_selftest,
        setup_window::server_open_setup,
        labs::deploying_labs,
        app::restart_app,
        labs::active_operations,
        labs::lab_deploy_log,
        app::linger_quit,
        app::force_quit,
        app::open_settings,
        app::open_log_folder,
        app::managed_settings,
    ])
}

#[cfg(test)]
mod tests {
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
        let registered = names_between(include_str!("commands.rs"), "generate_handler![", "])");
        let manifest = names_between(include_str!("../build.rs"), "const COMMANDS: &[&str] = &[", "];");
        assert!(registered.len() > 50, "{registered:?}");
        assert_eq!(manifest, registered);
    }

    /// Commands no unit test can call, each with why. Everything else in build.rs's list is
    /// called by name from a test.
    const NOT_UNIT_TESTED: &[(&str, &str)] = &[
        ("auth_login", "opens the system browser and waits for cyber-auth's redirect; authorize_url and the callback are tested"),
        ("auth_login_device", "polls cyber-auth's device endpoint; login_by_device_at is tested against a local cyber-auth"),
        ("restart_app", "restarts this very process"),
        ("open_log_folder", "opens the system's file manager; log_dir is tested"),
        ("lab_requirements", "fetches the spec at the pinned commit (spec_at downloads it); its sizing, lab::requirements, is unit-tested"),
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
        let tests: String = crate::testing::source_scan::crate_sources().into_iter().map(|s| s.tests + "\n").collect();
        let exempt = |c: &str| NOT_UNIT_TESTED.iter().any(|(n, _)| *n == c);
        let untested: Vec<&String> = commands.iter().filter(|c| !exempt(c) && !calls(&tests, c)).collect();
        assert!(untested.is_empty(), "commands no test calls (test them, or add them to NOT_UNIT_TESTED with why): {untested:?}");
        for (name, _) in NOT_UNIT_TESTED {
            assert!(commands.contains(*name), "{name} is no longer a command");
        }
    }

    /// What opens a window: Tauri's builder, the app's own, and the helpers built on them.
    const OPENS_A_WINDOW: &[&str] = &["WebviewWindowBuilder::new(", "window::builder(", "open_shell(", "open_settings_window(", "setup_window::open("];

    /// The synchronous commands in `code` (named after `pub fn`) whose body opens a window.
    fn sync_commands_opening_windows(code: &str) -> Vec<String> {
        code.split("#[tauri::command]")
            .skip(1)
            .filter_map(|after| {
                let item = after.trim_start().lines().skip_while(|l| l.trim_start().starts_with("#[")).collect::<Vec<_>>().join("\n");
                let name = item.strip_prefix("pub fn ")?.split('(').next()?.to_owned();
                let body = item.split("\n}").next().unwrap_or_default();
                OPENS_A_WINDOW.iter().any(|o| body.contains(o)).then_some(name)
            })
            .collect()
    }

    /// A synchronous command runs on the main thread, and a window it opens on Windows waits
    /// for that thread forever: the window stays white (Tauri's docs, wry#583).
    #[test]
    fn every_command_opening_a_window_is_async() {
        let sync: Vec<String> = crate::testing::source_scan::crate_sources().iter().flat_map(|s| sync_commands_opening_windows(&s.code)).collect();
        assert!(sync.is_empty(), "make these commands async: {sync:?}");
    }

    #[test]
    fn a_sync_command_opening_a_window_is_found() {
        let code = "#[tauri::command]\npub fn shell(app: AppHandle) -> Result<()> {\n    window::open_shell(&app)\n}\n\
                    #[tauri::command]\n#[cfg(unix)]\npub fn other(app: AppHandle) {\n    ok()\n}\n\
                    #[tauri::command]\npub async fn settings(app: AppHandle) {\n    open_settings_window(&app)\n}\n";
        assert_eq!(sync_commands_opening_windows(code), ["shell"]);
    }

    #[test]
    fn a_call_is_told_apart_from_a_longer_name() {
        assert!(calls("let x = lab_status (app, id);", "lab_status"));
        assert!(!calls("server_lab_status(app)", "lab_status"));
        assert!(!calls("\"lab_status\",", "lab_status"));
        assert!(calls("InvokeRequest { cmd: \"lab_status\".into(), ..", "lab_status"));
    }
}
