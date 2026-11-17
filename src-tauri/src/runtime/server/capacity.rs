//! A Proxmox host's hardware headroom, shown on its row so the player can see what it can run.

use serde::Serialize;

use super::profile::HostProfile;
use super::secrets::get_secret;
use super::store::load_host;
use crate::error::{Error, Result};
use crate::platform::AppDirs;
use crate::runtime::providers::Provider;
use isothrum_machine::place::proxmox::{self, Capacity};

use super::tls_pin;

/// A host's cores and memory (bytes), as its node reports them.
#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct HostCapacity {
    /// CPU cores.
    pub cores: u64,
    /// Memory in all, in bytes.
    pub mem_total: u64,
    /// Memory free, in bytes.
    pub mem_free: u64,
}

/// The capacity of the saved host `id`, read from its API (cheap calls): `None` for a host that
/// isn't a Proxmox server, an error when it couldn't be read.
pub(crate) async fn capacity(dirs: &AppDirs, id: &str) -> Result<Option<HostCapacity>> {
    let host = load_host(dirs, id)?;
    if host.provider != Provider::Proxmox {
        return Ok(None);
    }
    let password = get_secret(id)?;
    proxmox_capacity(&host, &password).await.map(Some)
}

/// Signs in to Proxmox server `h` with `password` and reads its node's cores and memory, through
/// isothrum-machine's Proxmox client ([`proxmox::sign_in`], [`Capacity::of`]).
async fn proxmox_capacity(h: &HostProfile, password: &str) -> Result<HostCapacity> {
    let server = proxmox::Server::new(h.host.as_str(), h.port);
    let session = proxmox::sign_in(&server, &proxmox::Credentials::from_login(&h.username, password), h.tls_trust()).await.map_err(|e| match e {
        proxmox::SignInError::Certificate(refused) => tls_pin::refused(refused, &h.name),
        other => Error::Invalid(format!("couldn't sign in to the Proxmox server: {other}")),
    })?;
    let node = session.node(h.node.as_deref()).await.map_err(|e| Error::Invalid(format!("couldn't list the Proxmox server's nodes: {e}")))?;
    let status = session.status(&node).await.map_err(|e| Error::Invalid(format!("couldn't read the node {node}'s status: {e}")))?;
    let c = Capacity::of(&status).ok_or_else(|| Error::Invalid(format!("the node {node} reported no cores or memory figures")))?;
    Ok(HostCapacity { cores: c.cores, mem_total: c.mem_total, mem_free: c.mem_free })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn only_a_saved_proxmox_server_reports_its_capacity_and_a_failed_read_says_why() {
        let t = crate::testing::test_app::TestApp::new();
        let az = t.host("az1", "azure", serde_json::json!({}));
        assert!(capacity(&t.dirs(), &az).await.unwrap().is_none(), "only Proxmox reports capacity");
        assert!(capacity(&t.dirs(), "nope").await.is_err(), "not saved");
        // Saved with no secret: the read fails and says why, rather than reading as no figure.
        #[cfg(debug_assertions)]
        {
            crate::runtime::server::secrets::use_test_file(t.data().join("secrets.json"));
            let pve = t.host("pve1", "proxmox", serde_json::json!({ "host": "127.0.0.1", "port": 1 }));
            assert_eq!(capacity(&t.dirs(), &pve).await.err().map(|e| e.code()), Some(crate::error::Code::NoPasswordStored));
        }
    }
}
