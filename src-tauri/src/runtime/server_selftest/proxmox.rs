//! The Proxmox self-test, connection-only: reach the node over the Proxmox API and confirm it,
//! which proves the endpoint, the API token and the node name. Labs themselves run through the
//! Isothrum runner now, so the launcher no longer stands up a throwaway VM from its own Terraform
//! to test a host.

use std::path::Path;

use crate::AppHandle;

use super::{Ran, Reporter};
use crate::runtime::server;

/// Nothing is created connection-only, so there is never a VM left to remove.
pub(super) fn leftover(_r: &Reporter, _conn: &server::Connection, _work: &Path) {}

pub(super) async fn run(app: &AppHandle, id: &str, r: &Reporter, _conn: &server::Connection, _work: &Path) -> Ran {
    let result = r
        .step("node", "Reach the Proxmox node over the API", async {
            match server::discovered_node(app, id).await {
                Ok(Some(node)) => Ok(((), Some(format!("node {node}")))),
                Ok(None) => Ok(((), Some("the API answered".into()))),
                Err(e) => Err(e),
            }
        })
        .await;
    // Connection-only: nothing was created, so there is nothing to clean up.
    Ran { result, cleaned: true }
}
