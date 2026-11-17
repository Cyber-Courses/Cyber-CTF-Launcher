//! Where a Proxmox host can put labs, for the setup's Placement step: isothrum-machine reads the
//! nodes, and on each the storages for VM disks, cloud images and cloud-init snippets and the
//! bridges (with their addresses), each with a sensible default (`place::proxmox::placement`).

use std::net::IpAddr;

pub use isothrum_machine::place::proxmox::placement::Placement;

use super::{HostProfile, SignInError, said, sign_in};

/// Signs in to `h` with `secret` and reads its choices; `server_ip` is the server's address as
/// this machine resolves it (to pick the bridge it is on).
pub async fn read_host(h: &HostProfile, secret: &str, server_ip: Option<IpAddr>) -> crate::error::Result<Placement> {
    const CONTEXT: &str = "Couldn't read the server's nodes, storages and bridges";
    let session = sign_in(h, secret).await.map_err(|e| match e {
        SignInError::Rejected => crate::error::Error::Invalid(format!("{CONTEXT}: Proxmox rejected the sign-in")),
        e => e.into_error(CONTEXT),
    })?;
    isothrum_machine::place::proxmox::placement::read(session.api(), &isothrum_machine::place::proxmox::check::Needs::default(), server_ip)
        .await
        .map_err(|e| crate::error::Error::Invalid(format!("{CONTEXT}: {}", said(&e))))
}
