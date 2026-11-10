//! A Proxmox host's hardware headroom, shown on its row so the player can see what it can run.

use serde::Serialize;

use super::profile::HostProfile;
use super::secrets::get_secret;
use super::store::load_host;
use crate::error::{Error, Result};
use crate::platform::AppDirs;
use crate::runtime::providers::Provider;

/// A host's cores and memory (bytes), as its node reports them.
#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct HostCapacity {
    pub cores: u64,
    pub mem_total: u64,
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

async fn proxmox_capacity(h: &HostProfile, password: &str) -> Result<HostCapacity> {
    let session = crate::runtime::proxmox::sign_in(h, password).await.map_err(|e| Error::Invalid(format!("couldn't sign in to the Proxmox server: {e}")))?;
    let node = session.node(h).await.map_err(|e| Error::Invalid(format!("couldn't list the Proxmox server's nodes: {e}")))?;
    let status = session.call(&format!("/nodes/{node}/status")).await.map_err(|e| Error::Invalid(format!("couldn't read the node {node}'s status: {e}")))?;
    capacity_from(&status).ok_or_else(|| Error::Invalid(format!("the node {node} reported no cores or memory figures")))
}

/// A Proxmox node's `/status` as the row's capacity (None when a figure is missing).
fn capacity_from(d: &serde_json::Value) -> Option<HostCapacity> {
    Some(HostCapacity { cores: d["cpuinfo"]["cpus"].as_u64()?, mem_total: d["memory"]["total"].as_u64()?, mem_free: d["memory"]["free"].as_u64()? })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capacity_needs_every_figure() {
        let d = serde_json::json!({"cpuinfo": {"cpus": 8}, "memory": {"total": 64, "free": 20}});
        let c = capacity_from(&d).unwrap();
        assert_eq!((c.cores, c.mem_total, c.mem_free), (8, 64, 20));
        assert!(capacity_from(&serde_json::json!({"cpuinfo": {"cpus": 8}})).is_none());
    }

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
