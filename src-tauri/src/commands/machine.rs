//! This machine's commands: its report and metrics, the setup self-tests, the engines, the
//! workloads and storage the Machine page shows, and the tools Cyber CTF installs.

use tauri::ipc::Channel;

use crate::AppHandle;
use crate::error::Result;
use crate::machine::images;
use crate::machine::selftest::{self, Event, Kind};
use crate::machine::storage::{self, Storage};
use crate::machine::system::metrics::{self, MachineMetrics};
use crate::machine::system::{self, SystemReport, docker, setup_window, wsl};
use crate::machine::tools::install::{self, Dependency};
use crate::machine::tools::uninstall::{self, InstalledTool};
use crate::machine::workloads::{self, Workload};
use crate::runtime::providers::Provider;

use super::lines;

/// What this machine has installed and can run (see [`system::system_check`]).
#[tauri::command]
pub async fn system_check(app: AppHandle) -> SystemReport {
    system::system_check(app).await
}

/// This machine's CPU, memory and disk use now.
#[tauri::command]
pub async fn machine_metrics() -> MachineMetrics {
    metrics::machine_metrics().await
}

/// Opens the machine setup window, at `step` when given.
#[tauri::command]
pub async fn machine_open_setup(app: AppHandle, step: Option<String>) -> Result<()> {
    setup_window::machine_open_setup(app, step)
}

/// Restarts the computer, for `reason` (WSL's install finishing).
#[tauri::command]
pub async fn machine_restart(app: AppHandle, reason: String) -> Result<()> {
    wsl::machine_restart(app, reason).await
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

/// Runs the setup self-test of `kind`, its steps sent on `events` (see [`selftest::machine_selftest`]).
#[tauri::command]
pub async fn machine_selftest(app: AppHandle, kind: Kind, provider: Option<Provider>, events: Channel<Event>) -> Result<()> {
    Box::pin(selftest::machine_selftest(app, kind, provider, events)).await
}

/// Downloads what the self-test of `kind` needs ahead of it.
#[tauri::command]
pub async fn machine_selftest_prefetch(app: AppHandle, kind: Kind, provider: Option<Provider>) -> Result<()> {
    selftest::machine_selftest_prefetch(app, kind, provider).await
}

/// The labs and attack boxes running on this machine.
#[tauri::command]
pub async fn machine_workloads(app: AppHandle) -> Vec<Workload> {
    workloads::machine_workloads(app).await
}

/// Stops the workload `id` of `kind`.
#[tauri::command]
pub async fn machine_workload_stop(app: AppHandle, kind: String, id: String) -> Result<()> {
    workloads::machine_workload_stop(app, kind, id).await
}

/// What Cyber CTF's labs and images take on disk.
#[tauri::command]
pub async fn machine_storage(app: AppHandle, extra_images: Vec<String>) -> Result<Storage> {
    storage::machine_storage(app, extra_images).await
}

/// Removes what Cyber CTF stored that no lab needs now; the bytes freed.
#[tauri::command]
pub async fn machine_storage_clean(app: AppHandle, extra_images: Vec<String>) -> Result<u64> {
    storage::machine_storage_clean(app, extra_images).await
}

/// Installs `dependency`, each command and its output sent on `logs`.
#[tauri::command]
pub async fn install_dependency(app: AppHandle, dependency: Dependency, logs: Channel<String>) -> Result<()> {
    install::install_dependency(app, dependency, lines(logs)).await
}

/// Installs the Vagrant plugin `plugin`, its output sent on `logs`.
#[tauri::command]
pub async fn install_vagrant_plugin(plugin: String, logs: Channel<String>) -> Result<()> {
    install::install_vagrant_plugin(plugin, lines(logs)).await
}

/// The tools Cyber CTF installed, which Cleanup may remove.
#[tauri::command]
pub async fn installed_tools(app: AppHandle) -> Vec<InstalledTool> {
    uninstall::installed_tools(app).await
}

/// Removes `dependency`, which Cyber CTF installed, its output sent on `logs`.
#[tauri::command]
pub async fn uninstall_dependency(app: AppHandle, dependency: Dependency, logs: Channel<String>) -> Result<()> {
    uninstall::uninstall_dependency(app, dependency, lines(logs)).await
}
