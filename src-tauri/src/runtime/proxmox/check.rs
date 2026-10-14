//! "Test connection" for a Proxmox host: sign-in, node, storages, the lab bridge and, for a
//! token setup, the launcher's SSH key, each problem with its fix.

use super::*;

/// The outcome of "Test connection" for a Proxmox host.
pub struct Report {
    pub ok: bool,
    pub authenticated: Option<bool>,
    pub message: String,
}

pub(super) fn fail(authenticated: Option<bool>, message: impl Into<String>) -> Report {
    Report { ok: false, authenticated, message: message.into() }
}

/// What a storage accepts, from `/nodes/<node>/storage` (`content` is comma-separated).
pub(super) fn storage_content(storages: &Value, id: &str) -> Option<Vec<String>> {
    storages
        .as_array()?
        .iter()
        .find(|s| s["storage"].as_str() == Some(id))
        .map(|s| s["content"].as_str().unwrap_or_default().split(',').map(|c| c.trim().to_string()).collect())
}

pub(super) const STORAGE_HINT: &str = "In the Proxmox web UI: Datacenter > Storage > select it > Edit > Content";

/// Missing storage content a lab launch needs, as fix-it sentences.
pub(super) fn storage_problems(storages: &Value, vm_storage: &str) -> Vec<String> {
    let mut needs: Vec<(&str, &str, &str)> =
        vec![(IMAGE_STORAGE, "iso", "ISO image"), (SNIPPET_STORAGE, "snippets", "Snippets"), (vm_storage, "images", "Disk image")];
    needs.dedup();
    let mut problems = Vec::new();
    for (id, content, label) in needs {
        match storage_content(storages, id) {
            None => problems.push(format!("Storage \"{id}\" doesn't exist on this node, or this user can't see it.")),
            Some(c) if !c.iter().any(|x| x == content) => problems.push(format!("Storage \"{id}\" doesn't accept {label}s. {STORAGE_HINT}, add '{label}'.")),
            Some(_) => {}
        }
    }
    problems
}

/// Signs in, then checks the node, the storages and bridge a launch uses and, for token
/// setups, that the launcher's SSH key is authorized on the node.
pub async fn test(h: &HostProfile, secret: &str) -> Report {
    let session = match sign_in(h, secret).await {
        Ok(s) => s,
        Err(e) => return sign_in_report(e, is_token(&h.username)),
    };
    let node = match session.node(h).await {
        Ok(n) => n,
        Err(e) => return node_report(e),
    };

    let mut problems = Vec::new();
    let vm_storage = h.datastore.clone().filter(|s| !s.is_empty()).unwrap_or_else(|| "local-lvm".into());
    match session.call(&format!("/nodes/{node}/storage")).await {
        Ok(storages) => problems.extend(storage_problems(&storages, &vm_storage)),
        Err(e) => problems.push(format!("Couldn't list the node's storage ({e}).")),
    }
    let bridge = lab_bridge(h);
    match session.call(&format!("/nodes/{node}/network")).await {
        Ok(ifaces) => problems.extend(bridge_problem(&ifaces, &bridge, &node)),
        Err(e) => problems.push(format!("Couldn't list the node's network ({e}).")),
    }
    if is_token(&h.username)
        && let Some(problem) = ssh_key_problem(h).await
    {
        problems.push(problem);
    }
    outcome(&node, &bridge, &problems)
}

/// "Test connection" when signing in failed.
pub(super) fn sign_in_report(e: SignInError, token: bool) -> Report {
    match e {
        SignInError::Rejected if token => fail(Some(false), "Proxmox rejected the API token. Check the token id (user@realm!name) and its secret."),
        SignInError::Rejected => fail(Some(false), "Proxmox rejected the credentials. Use user@realm, e.g. root@pam."),
        SignInError::Failed(e) => fail(None, format!("Reachable, but the API call failed: {e}")),
    }
}

/// "Test connection" when listing the nodes failed.
pub(super) fn node_report(e: CallError) -> Report {
    match e {
        CallError::Status(403) => fail(
            Some(true),
            "Signed in, but this user or token can't list nodes. Give it the Administrator role, or create the token with privilege separation off.",
        ),
        e => fail(Some(true), format!("Signed in, but listing nodes failed: {e}")),
    }
}

/// The lab bridge missing from the node's interfaces, as a fix-it sentence.
pub(super) fn bridge_problem(ifaces: &Value, bridge: &str, node: &str) -> Option<String> {
    find_iface(ifaces, bridge)
        .is_none()
        .then(|| format!("Bridge \"{bridge}\" doesn't exist on {node}. Use one of the node's bridges (System > Network), e.g. vmbr0."))
}

/// "Test connection" once signed in: ready, or every problem a launch would hit.
pub(super) fn outcome(node: &str, bridge: &str, problems: &[String]) -> Report {
    if problems.is_empty() {
        Report {
            ok: true,
            authenticated: Some(true),
            message: format!("Signed in to the Proxmox API; node {node}, storage and bridge {bridge} are ready for labs."),
        }
    } else {
        fail(Some(true), format!("Signed in, but a lab launch would fail:\n• {}", problems.join("\n• ")))
    }
}

/// Whether the host's lab bridge is host-internal (no physical port, e.g. a NAT bridge for
/// lab VMs): this machine can't route to VMs on it, only the node can. Not knowing is an error,
/// never a "no": that guess sent Terraform straight at a VM it can't reach (where it hung) and
/// dropped the jump host of a lab already running.
pub async fn bridge_is_internal(h: &HostProfile, secret: &str) -> crate::error::Result<bool> {
    let session = sign_in(h, secret).await.map_err(bridge_unknown)?;
    let node = session.node(h).await.map_err(bridge_unknown)?;
    let ifaces = session.call(&format!("/nodes/{node}/network")).await.map_err(bridge_unknown)?;
    Ok(find_iface(&ifaces, &lab_bridge(h)).is_some_and(has_no_ports))
}

/// Why the lab bridge couldn't be checked, as the launch's error.
fn bridge_unknown(why: impl std::fmt::Display) -> crate::error::Error {
    crate::error::Error::Invalid(format!(
        "Couldn't check how this machine reaches the Proxmox lab network ({why}). Check the host with Test connection in Servers, then try again."
    ))
}
