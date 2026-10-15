//! The labs' commands: starting a lab, and the operations in flight.

use tauri::ipc::Channel;

use crate::AppHandle;
use crate::error::Result;
use crate::labs::operations::{self, ActiveOperation};
use crate::labs::{self, LaunchRequest};
use crate::platform::AppDirs;
use crate::runtime::providers::Provider;

/// Starts a lab, the deploy's log sent on `logs` (see [`labs::lab_launch`]).
#[tauri::command]
#[allow(clippy::too_many_arguments, reason = "the webview sends each of the request's fields as an argument")]
pub async fn lab_launch(
    app: AppHandle,
    lab_id: String,
    provider: Option<Provider>,
    host: Option<String>,
    attackbox_image: Option<String>,
    default_ports: Option<bool>,
    offline: Option<bool>,
    logs: Channel<String>,
) -> Result<()> {
    let request = LaunchRequest { lab_id, provider, host, attackbox_image, default_ports: default_ports.unwrap_or(false), offline: offline.unwrap_or(false) };
    labs::lab_launch(app, request, logs).await
}

/// The labs a deploy worker is running an operation on.
#[tauri::command]
pub async fn deploying_labs(app: AppHandle) -> Vec<String> {
    operations::deploying_labs(AppDirs::from(&app)).await
}

/// Every lab operation in flight, here or in a deploy worker.
#[tauri::command]
pub async fn active_operations(app: AppHandle) -> Vec<ActiveOperation> {
    operations::active_operations(AppDirs::from(&app)).await
}

/// The log of lab `id`'s last deploy.
#[tauri::command]
pub async fn lab_deploy_log(app: AppHandle, id: String) -> std::result::Result<String, String> {
    operations::lab_deploy_log(AppDirs::from(&app), id).await
}
