//! This machine's commands: its report and metrics, the setup self-tests, the engines, the
//! workloads and storage the Machine page shows, and the tools Cyber CTF installs.

use std::future::Future;

use tauri::ipc::Channel;

use crate::AppHandle;
use crate::app::setup_window;
use crate::error::{Error, Result};
use crate::machine::images;
use crate::machine::selftest::{self, Kind};
use crate::machine::setup::WindowsFeature;
use crate::machine::storage::{self, Storage};
use crate::machine::system::metrics::{self, MachineMetrics};
use crate::machine::system::{self, SystemReport, docker, wsl};
use crate::machine::tools::install::{self, Dependency};
use crate::machine::tools::ovftool;
use crate::machine::tools::uninstall::{self, Installed};
use crate::machine::workloads::{self, Workload};
use crate::platform::AppDirs;
use crate::runtime::Runtime;
use crate::runtime::providers::Provider;
use crate::runtime::selftest_report::Event;

use super::lines;

/// What this machine has installed and can run (see [`system::system_check`]).
#[tauri::command]
pub async fn system_check(app: AppHandle) -> SystemReport {
    system::system_check(AppDirs::from(&app)).await
}

/// This machine's CPU, memory and disk use now.
#[tauri::command]
pub async fn machine_metrics() -> MachineMetrics {
    metrics::machine_metrics().await
}

/// Opens the machine setup window, at `step` when given, for the hypervisor `provider` when
/// given (a Machine page row's Fix).
#[tauri::command]
pub async fn machine_open_setup(app: AppHandle, step: Option<String>, provider: Option<crate::runtime::providers::Provider>) -> Result<()> {
    setup_window::open(&app, step.as_deref(), provider)
}

/// Restarts the computer, for `reason`: turning `feature` on finishes (WSL 2, Hyper-V).
#[tauri::command]
pub async fn machine_restart(app: AppHandle, reason: String, feature: WindowsFeature) -> Result<()> {
    wsl::machine_restart(AppDirs::from(&app), reason, feature).await
}

/// Makes `engine` Docker's current context.
#[tauri::command]
pub async fn docker_use_engine(engine: String) -> Result<()> {
    docker::docker_use_engine(engine).await
}

/// Starts the Docker engine `engine`.
#[tauri::command]
pub async fn docker_start_engine(engine: String) -> Result<()> {
    docker::docker_start_engine(engine).await
}

/// How much `image` would download, when the registry says.
#[tauri::command]
pub async fn image_download_size(image: String) -> Option<u64> {
    images::image_download_size(image).await
}

/// Runs the setup self-test of `kind`, its steps sent on `events` (see [`selftest::machine_selftest`]):
/// on `provider` for a VM, on the Docker CLI `context` (one engine among several) for containers.
#[tauri::command]
pub async fn machine_selftest(app: AppHandle, kind: Kind, provider: Option<Provider>, context: Option<String>, events: Channel<Event>) -> Result<()> {
    selftest::machine_selftest(AppDirs::from(&app), kind, provider, context, move |e| {
        let _ = events.send(e);
    })
    .await
}

/// Downloads what the self-test of `kind` needs ahead of it.
#[tauri::command]
pub async fn machine_selftest_prefetch(app: AppHandle, kind: Kind, provider: Option<Provider>) -> Result<()> {
    selftest::machine_selftest_prefetch(AppDirs::from(&app), kind, provider).await
}

/// The labs running on this machine.
#[tauri::command]
pub async fn machine_workloads(app: AppHandle) -> Result<Vec<Workload>> {
    workloads::machine_workloads(AppDirs::from(&app)).await
}

/// Stops the workload `id` of `kind`.
#[tauri::command]
pub async fn machine_workload_stop(app: AppHandle, kind: workloads::WorkloadKind, id: String) -> Result<()> {
    workloads::machine_workload_stop(&app, kind, id).await
}

/// The runtime's labs, for the Machine page.
impl workloads::LabRuntime for AppHandle {
    fn runtime_here(&self, id: &str, in_vm: bool) -> impl Future<Output = Option<Runtime>> + Send {
        let dirs = AppDirs::from(self);
        let id = id.to_owned();
        async move { crate::runtime::runtime_here(&dirs, &id, in_vm).await }
    }

    fn stop_lab(&self, id: &str, runtime: Runtime) -> impl Future<Output = Result<()>> + Send {
        let app = self.clone();
        let request = crate::run_service::Request::Stop { lab_id: id.to_owned(), runtime };
        async move { crate::app::runs::run(&app, request).await }
    }
}

/// What Cyber CTF's labs and images take on disk.
#[tauri::command]
pub async fn machine_storage(app: AppHandle, extra_images: Vec<String>) -> Result<Storage> {
    storage::machine_storage(AppDirs::from(&app), extra_images).await
}

/// Removes what Cyber CTF stored that no lab needs now; the bytes freed.
#[tauri::command]
pub async fn machine_storage_clean(app: AppHandle, extra_images: Vec<String>) -> Result<u64> {
    storage_clean_in(AppDirs::from(&app), extra_images).await
}

/// [`machine_storage_clean`], in the app whose folders are `dirs`.
pub(crate) async fn storage_clean_in(dirs: AppDirs, extra_images: Vec<String>) -> Result<u64> {
    storage::machine_storage_clean(dirs, extra_images).await
}

/// Installs `dependency`, each command and its output sent on `logs`.
#[tauri::command]
pub async fn install_dependency(app: AppHandle, dependency: Dependency, logs: Channel<String>) -> Result<()> {
    install::install_dependency(AppDirs::from(&app), dependency, lines(logs)).await
}

/// Installs the Vagrant plugin `plugin`, its output sent on `logs`.
#[tauri::command]
pub async fn install_vagrant_plugin(plugin: String, logs: Channel<String>) -> Result<()> {
    install::install_vagrant_plugin(plugin, lines(logs)).await
}

/// The tools Cyber CTF installed, which Cleanup may remove.
#[tauri::command]
pub async fn installed_tools(app: AppHandle) -> Vec<Installed> {
    uninstall::installed_tools(AppDirs::from(&app)).await
}

/// Removes `dependency`, which Cyber CTF installed, its output sent on `logs`.
#[tauri::command]
pub async fn uninstall_dependency(app: AppHandle, dependency: Dependency, logs: Channel<String>) -> Result<()> {
    uninstall::uninstall_dependency(AppDirs::from(&app), dependency, lines(logs)).await
}

/// Places the OVF Tool unpacked at `folder` (which the player picked) in the app's own folder,
/// once verified as VMware's (see [`ovftool::place`]).
#[tauri::command]
pub async fn ovftool_place(app: AppHandle, folder: String) -> Result<ovftool::Outcome> {
    ovftool::place(&AppDirs::from(&app), std::path::Path::new(&folder)).await
}

/// Adds this Windows account to "Hyper-V Administrators" (one administrator prompt), so Vagrant
/// can drive Hyper-V for it from the next sign-in on (#185): isothrum-machine's
/// [`join_hyperv_admins`](isothrum_machine::setup::join_hyperv_admins). Refused off Windows.
#[tauri::command]
pub async fn hyperv_join_admins() -> Result<()> {
    use isothrum_machine::install::{StepFailed, exit_codes::Known};
    let cx = crate::runtime::providers::machine();
    if cx.host.os != isothrum_machine::HostOs::Windows {
        return Err(Error::Invalid("Hyper-V is only on Windows.".into()));
    }
    isothrum_machine::setup::join_hyperv_admins(&cx, &mut |_| {}).await.map_err(|failed| match failed {
        StepFailed::Exited { known: Some(Known::Cancelled), .. } => {
            Error::Invalid("The Windows permission prompt was declined, so nothing was changed. Try again and choose Yes.".into())
        }
        failed => Error::Invalid(format!("Windows couldn't add your account to Hyper-V Administrators ({failed}).")),
    })
}

#[cfg(test)]
mod tests {
    use tauri::Manager;

    use super::*;
    use crate::testing::test_app::TestApp;

    /// Nothing downloaded yet: nothing listed and nothing removed; Docker and Vagrant are asked
    /// only what they hold (an unscripted call fails the test).
    #[tokio::test]
    async fn nothing_downloaded_is_listed_nor_removed_through_the_commands() {
        use crate::testing::tools::{Tools, ok};
        let t = TestApp::new();
        let tools = Tools::strict_rules(&[("docker image ls -q --no-trunc", ok("")), ("vagrant box list", ok(""))]);
        let stored = machine_storage(t.handle().clone(), Vec::new()).await.unwrap();
        assert!(stored.images.is_empty() && stored.boxes.is_empty());
        assert_eq!(machine_storage_clean(t.handle().clone(), Vec::new()).await.unwrap(), 0);
        assert!(tools.calls().iter().all(|c| c.starts_with("docker image ls") || c.starts_with("vagrant box list")), "nothing removed: {:?}", tools.calls());
    }

    /// Opening a new window sizes it to the screen, which Tauri's mock runtime can't tell: the
    /// open one is driven here.
    #[tokio::test]
    async fn the_open_setup_window_is_brought_forward() {
        let t = TestApp::new();
        crate::app::window::builder(t.handle(), "machine-setup", "machine-setup").unwrap().build().unwrap();
        machine_open_setup(t.handle().clone(), Some("docker".into()), None).await.unwrap();
        assert_eq!(t.handle().webview_windows().len(), 1);
    }

    /// A picked folder that isn't the OVF Tool is refused, said as such, and nothing is placed.
    #[tokio::test]
    async fn placing_a_folder_without_the_ovf_tool_is_refused() {
        let t = TestApp::new();
        let picked = t.data().join("not-the-tool");
        std::fs::create_dir_all(&picked).unwrap();
        let outcome = ovftool_place(t.handle().clone(), picked.display().to_string()).await.unwrap();
        assert_eq!(outcome, ovftool::Outcome::Refused { reason: isothrum_machine::tools::ovftool::Refusal::NoTool, detail: None });
        assert!(!ovftool::managed_dir(t.data()).exists());
    }

    #[tokio::test]
    async fn joining_hyperv_administrators_is_windows_only() {
        if !cfg!(windows) {
            let err = hyperv_join_admins().await.unwrap_err();
            assert!(err.to_string().contains("only on Windows"), "{err}");
        }
    }
}
