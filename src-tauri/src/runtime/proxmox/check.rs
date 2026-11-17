//! "Test connection" for a Proxmox host: sign-in, node, storages, the lab bridge and, for a
//! token setup, the launcher's SSH key, and the node's SDN config, each problem with its fix.
//! The checks are isothrum-machine's (`place::proxmox::check`); the sentences are the launcher's.

use isothrum_machine::place::proxmox::check::{self as checks, Content, Finding};

use super::{CallError, HostProfile, SignInError, is_token, lab_bridge, needs, said, sdn_problem, sign_in, ssh_key_problem};

/// The outcome of "Test connection" for a Proxmox host.
pub struct Report {
    /// Every check passed: the host is ready for labs.
    pub ok: bool,
    /// Whether the host accepted the credentials; None when the sign-in didn't get that far.
    pub authenticated: Option<bool>,
    /// What was found, or the problem with its fix.
    pub message: String,
}

pub(super) fn fail(authenticated: Option<bool>, message: impl Into<String>) -> Report {
    Report { ok: false, authenticated, message: message.into() }
}

pub(super) const STORAGE_HINT: &str = "In the Proxmox web UI: Datacenter > Storage > select it > Edit > Content";

/// A content's name in the Proxmox web UI's Content list.
fn label(content: Content) -> &'static str {
    match content {
        Content::Iso => "ISO image",
        Content::Snippets => "Snippets",
        Content::Images => "Disk image",
    }
}

/// A finding of the node's checks as a fix-it sentence (`node` names it).
pub(super) fn problem(finding: &Finding, node: &str) -> String {
    match finding {
        Finding::NoStorage { storage } => format!("Storage \"{storage}\" doesn't exist on this node, or this user can't see it."),
        Finding::StorageLacks { storage, content } => {
            let label = label(*content);
            format!("Storage \"{storage}\" doesn't accept {label}s. {STORAGE_HINT}, add '{label}'.")
        }
        Finding::NoBridge { bridge } => format!("Bridge \"{bridge}\" doesn't exist on {node}. Use one of the node's bridges (System > Network), e.g. vmbr0."),
        Finding::StoragesUnread(e) => format!("Couldn't list the node's storage ({}).", said(e)),
        Finding::NetworkUnread(e) => format!("Couldn't list the node's network ({}).", said(e)),
        other => format!("{other:?}"),
    }
}

/// Missing storage content a lab launch needs, as fix-it sentences.
#[cfg(test)]
pub(super) fn storage_problems(storages: &[super::api::Storage], vm_storage: &str, image_storage: &str, snippet_storage: &str) -> Vec<String> {
    let needs =
        checks::Needs { disk_storage: vm_storage.into(), image_storage: image_storage.into(), snippet_storage: snippet_storage.into(), bridge: String::new() };
    checks::storage_findings(storages, &needs).iter().map(|f| problem(f, "")).collect()
}

/// Signs in, then checks the node, the storages and bridge a launch uses and, for token
/// setups, that the launcher's SSH key (`identity`, in the app's own folder) is authorized on the
/// node.
pub async fn test(h: &HostProfile, secret: &str, identity: &std::path::Path) -> Report {
    let session = match sign_in(h, secret).await {
        Ok(s) => s,
        Err(e) => return sign_in_report(e, h),
    };
    let node = match session.node(h).await {
        Ok(n) => n,
        Err(e) => return node_report(e),
    };
    let needs = needs(h);
    let mut problems: Vec<String> = checks::check(session.api(), &node, &needs).await.iter().map(|f| problem(f, &node)).collect();
    if is_token(&h.username)
        && let Some(problem) = ssh_key_problem(h, identity).await
    {
        problems.push(problem);
    }
    // Lab networks are SDN vnets: the node must source its SDN config, or their bridges never exist.
    problems.extend(sdn_problem(h, secret, identity).await);
    outcome(&node, &needs.bridge, &problems)
}

/// "Test connection" when signing in to host `h` failed.
pub(super) fn sign_in_report(e: SignInError, h: &HostProfile) -> Report {
    match e {
        SignInError::Rejected if is_token(&h.username) => {
            fail(Some(false), "Proxmox rejected the API token. Check the token id (user@realm!name) and its secret.")
        }
        SignInError::Rejected => fail(Some(false), "Proxmox rejected the credentials. Use user@realm, e.g. root@pam."),
        SignInError::Certificate(refusal, server) => fail(None, super::tls_pin::refused(refusal, &server).to_string()),
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
        e => fail(Some(true), format!("Signed in, but listing nodes failed: {}", said(&e))),
    }
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
    let session = sign_in(h, secret).await.map_err(|e| match e {
        e @ SignInError::Certificate(..) => e.into_error("Couldn't sign in to the Proxmox server"),
        e => bridge_unknown(e),
    })?;
    let node = session.node(h).await.map_err(bridge_unknown)?;
    checks::bridge_is_internal(session.api(), &node, &lab_bridge(h)).await.map_err(|e| bridge_unknown(said(&e)))
}

/// Why the lab bridge couldn't be checked, as the launch's error.
fn bridge_unknown(why: impl std::fmt::Display) -> crate::error::Error {
    crate::error::Error::Invalid(format!(
        "Couldn't check how this machine reaches the Proxmox lab network ({why}). Check the host with Test connection in Servers, then try again."
    ))
}
