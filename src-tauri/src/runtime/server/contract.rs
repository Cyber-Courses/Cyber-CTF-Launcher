//! What a lab is given to reach its server: the env a Vagrantfile reads, Terraform's env
//! and variables, and which server a lab directory is tied to.

use std::path::Path;

use crate::AppHandle;

use super::HOST_MARKER;
use super::kind::kind;
use super::profile::{DEFAULT_AUTO_STOP_HOURS, HostProfile};
use super::secrets::{get_secret, host_secret};
use super::store::{Store, load, load_host, saved_host};
use crate::error::Result;
use crate::runtime::lab;
use crate::runtime::providers::Provider;
use crate::runtime::proxmox;

// --- the Vagrantfile contract ---------------------------------------------

/// Environment a lab's Vagrantfile reads to target this host. See `docs/server.md`.
/// ESXi Vagrantfiles should use `esxi.esxi_password = "env:CYBERCTF_ESXI_PASSWORD"` so the
/// plugin reads the secret itself.
pub fn connection_env(h: &HostProfile, password: &str) -> Vec<(String, String)> {
    let mut env: Vec<(String, String)> = Vec::new();
    let mut set = |k: &str, v: String| env.push((k.to_string(), v));
    set("CYBERCTF_REMOTE_PROVIDER", h.provider.id().into());
    set("CYBERCTF_REMOTE_HOST", h.host.clone());
    match h.provider {
        Provider::VmwareEsxi => {
            // The launcher's names, then the ones Isoloom's Vagrantfiles read (same values).
            set("CYBERCTF_ESXI_HOSTNAME", h.host.clone());
            set("CYBERCTF_ESXI_HOSTPORT", h.port.to_string());
            set("CYBERCTF_ESXI_USERNAME", h.username.clone());
            set("CYBERCTF_ESXI_PASSWORD", password.into());
            if let Some(v) = &h.datastore {
                set("CYBERCTF_ESXI_DISK_STORE", v.clone());
            }
            if let Some(v) = &h.network {
                set("CYBERCTF_ESXI_VIRTUAL_NETWORK", v.clone());
            }
            env.extend(lab::esxi_env(h, password));
        }
        Provider::Proxmox => {
            set("CYBERCTF_PROXMOX_ENDPOINT", proxmox::api_url(h));
            set("CYBERCTF_PROXMOX_USER_NAME", h.username.clone());
            set("CYBERCTF_PROXMOX_PASSWORD", password.into());
            set("CYBERCTF_PROXMOX_INSECURE", h.insecure_tls.to_string());
            if let Some(v) = &h.node {
                set("CYBERCTF_PROXMOX_NODE", v.clone());
            }
            if let Some(v) = &h.datastore {
                set("CYBERCTF_PROXMOX_STORAGE", v.clone());
            }
            if let Some(v) = &h.network {
                set("CYBERCTF_PROXMOX_BRIDGE", v.clone());
            }
        }
        _ => {}
    }
    env
}

// --- Terraform --------------------------------------------------------------

/// Raw environment for Terraform (credentials the provider reads itself, never variables), by
/// the host's kind (see `kind`).
pub fn terraform_env(h: &HostProfile, password: &str) -> Vec<(String, String)> {
    kind(h.provider).map(|k| (k.terraform_env)(h, password)).unwrap_or_default()
}

fn pair(k: &str, v: &str) -> (String, String) {
    (k.to_string(), v.to_string())
}

/// A provider that reads nothing from the environment (or signs in with its own CLI's
/// credentials, like gcloud's ADC).
pub(super) fn no_env(_: &HostProfile, _: &str) -> Vec<(String, String)> {
    Vec::new()
}

/// AWS: the stored keys and the region; with the AWS CLI's own credentials, the region (and the
/// chosen profile): Terraform reads the CLI's chain (a local terraform has the host's ~/.aws).
pub(super) fn aws_env(h: &HostProfile, password: &str) -> Vec<(String, String)> {
    if !h.use_cli_creds {
        return vec![pair("AWS_ACCESS_KEY_ID", &h.username), pair("AWS_SECRET_ACCESS_KEY", password), pair("AWS_REGION", &h.host)];
    }
    let mut env = vec![pair("AWS_REGION", &h.host)];
    if let Some(p) = &h.aws_profile {
        env.push(pair("AWS_PROFILE", p));
    }
    env
}

/// azurerm uses the Azure CLI's auth (az login); it only needs the subscription id.
pub(super) fn azure_env(h: &HostProfile, _: &str) -> Vec<(String, String)> {
    vec![pair("ARM_SUBSCRIPTION_ID", &h.username)]
}

/// The digitalocean provider reads DIGITALOCEAN_TOKEN (the stored API token).
pub(super) fn digitalocean_env(_: &HostProfile, token: &str) -> Vec<(String, String)> {
    vec![pair("DIGITALOCEAN_TOKEN", token)]
}

/// The linode provider reads LINODE_TOKEN (the stored API token).
pub(super) fn linode_env(_: &HostProfile, token: &str) -> Vec<(String, String)> {
    vec![pair("LINODE_TOKEN", token)]
}

// --- resolving a host for a launch ------------------------------------------

/// A host resolved for a launch: its provider, the Vagrant env (Vagrantfile contract) and
/// the Terraform variables (Isoloom's modules, see `lab::host_vars`).
pub struct Connection {
    pub provider: Provider,
    pub name: String,
    pub env: Vec<(String, String)>,
    pub tf_vars: Vec<(String, String)>,
    /// Raw environment for Terraform (cloud credentials the provider reads itself).
    pub tf_env: Vec<(String, String)>,
    /// A cloud account's region (None for a server).
    pub region: Option<String>,
    /// A cloud account's auto-stop, in hours (0: none; None for a server).
    pub auto_stop_hours: Option<u32>,
    /// A GCP account's labs project, once it has one.
    pub gcp_project: Option<String>,
}

pub fn connection(app: &AppHandle, id: &str) -> Result<Connection> {
    let host = load_host(app, id)?;
    // CLI-credential hosts keep no secret; Terraform uses the CLI's own sign-in.
    let password = host_secret(&host)?;
    Ok(connection_for(&host, &password))
}

/// What a launch on `host` is given, with its secret.
fn connection_for(host: &HostProfile, password: &str) -> Connection {
    Connection {
        provider: host.provider,
        name: host.name.clone(),
        env: connection_env(host, password),
        tf_vars: lab::host_vars(host, password),
        tf_env: terraform_env(host, password),
        region: host.provider.is_cloud().then(|| host.host.clone()),
        auto_stop_hours: host.provider.is_cloud().then(|| host.auto_stop_hours.unwrap_or(DEFAULT_AUTO_STOP_HOURS)),
        gcp_project: host.gcp_project.clone().filter(|_| host.provider == Provider::Gcp),
    }
}

/// The player's hosts and cloud accounts as launch targets for the website
/// (`{ id, name, provider }`), reported by the launcher agent.
pub fn launch_targets(app: &AppHandle) -> Vec<serde_json::Value> {
    load(app).map(|s| targets_of(&s)).unwrap_or_default()
}

/// `launch_targets` of a loaded store.
fn targets_of(store: &Store) -> Vec<serde_json::Value> {
    store.hosts.iter().map(|h| serde_json::json!({ "id": h.id, "name": h.name, "provider": h.provider.id() })).collect()
}

/// A host's display name, if it exists.
pub fn host_name(app: &AppHandle, id: &str) -> Option<String> {
    saved_host(app, id).map(|h| h.name)
}

/// A host's provider, if it exists.
pub fn host_provider(app: &AppHandle, id: &str) -> Option<Provider> {
    saved_host(app, id).map(|h| h.provider)
}

/// The host marked as default, if any (used for VM labs launched from the website).
pub fn default_host(app: &AppHandle) -> Option<String> {
    default_of(load(app).ok()?)
}

/// `default_host` of a loaded store.
fn default_of(store: Store) -> Option<String> {
    // Only server hosts: a cloud account is never used implicitly (it costs money).
    store.default.filter(|id| store.hosts.iter().any(|h| &h.id == id && !h.provider.is_cloud()))
}

// --- labs tied to a host ----------------------------------------------------

/// Records (or clears) which host a VM lab directory runs on.
pub fn mark_lab(dir: &Path, host: Option<&str>) -> Result<()> {
    let marker = dir.join(HOST_MARKER);
    match host {
        Some(id) => std::fs::write(marker, id)?,
        None => {
            let _ = std::fs::remove_file(marker);
        }
    }
    Ok(())
}

/// The host id a lab directory is marked as running on.
pub fn marked_host(dir: &Path) -> Option<String> {
    std::fs::read_to_string(dir.join(HOST_MARKER)).ok().map(|id| id.trim().to_string())
}

/// For a Proxmox host whose lab bridge is host-internal: the node login to reach lab VMs
/// through, after making sure the launcher's key is accepted there. None otherwise.
pub async fn proxmox_jump(app: &AppHandle, dir: &Path, identity: &Path, public: &str) -> Result<Option<String>> {
    let Some(id) = marked_host(dir) else { return Ok(None) };
    proxmox_jump_for_host(app, &id, identity, public).await
}

/// `proxmox_jump` for a saved host id.
pub async fn proxmox_jump_for_host(app: &AppHandle, id: &str, identity: &Path, public: &str) -> Result<Option<String>> {
    let host = load_host(app, id)?;
    if host.provider != Provider::Proxmox {
        return Ok(None);
    }
    let secret = get_secret(id)?;
    if !proxmox::bridge_is_internal(&host, &secret).await? {
        return Ok(None);
    }
    if !proxmox::is_token(&host.username) {
        proxmox::authorize_launcher_key(&host, &secret, identity, public).await?;
    }
    Ok(Some(proxmox::node_login(&host)))
}

/// The connection a VM lab directory was started with, if it runs on a server host.
pub fn lab_connection(app: &AppHandle, dir: &Path) -> Result<Option<Connection>> {
    marked_host(dir).map(|id| connection(app, &id)).transpose()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::server::profile::tests::{get, profile as full_profile};

    /// A bare profile: no storage, network or node set.
    fn profile(provider: Provider) -> HostProfile {
        HostProfile { port: 8006, username: "root".into(), datastore: None, network: None, node: None, ..full_profile(provider) }
    }

    #[test]
    fn a_connection_gathers_env_vars_and_terraform_env() {
        let c = connection_for(&full_profile(Provider::Proxmox), "pw");
        assert_eq!((c.provider, c.name.as_str()), (Provider::Proxmox, "Lab"));
        assert_eq!(get(&c.env, "CYBERCTF_PROXMOX_PASSWORD"), Some("pw"));
        assert_eq!(get(&c.tf_vars, "proxmox_password"), Some("pw"));
        assert!(c.tf_env.is_empty());
        let c = connection_for(&HostProfile { host: "eu-west-3".into(), username: "AKIAEXAMPLE".into(), ..full_profile(Provider::Aws) }, "key");
        assert_eq!(get(&c.tf_env, "AWS_SECRET_ACCESS_KEY"), Some("key"));
        assert_eq!(get(&c.tf_vars, "region"), Some("eu-west-3"));
        // The launcher's own view of the account, not read back from Isoloom's variables.
        assert_eq!((c.region.as_deref(), c.auto_stop_hours, c.gcp_project.as_deref()), (Some("eu-west-3"), Some(DEFAULT_AUTO_STOP_HOURS), None));
        let c = connection_for(&HostProfile { auto_stop_hours: Some(0), gcp_project: Some("labs-1".into()), ..full_profile(Provider::Gcp) }, "");
        assert_eq!((c.auto_stop_hours, c.gcp_project.as_deref()), (Some(0), Some("labs-1")));
        let c = connection_for(&full_profile(Provider::Proxmox), "pw");
        assert_eq!((c.region, c.auto_stop_hours), (None, None));
    }

    #[test]
    fn launch_targets_list_every_host_and_account() {
        let store = Store {
            default: None,
            hosts: vec![HostProfile { id: "pve1".into(), ..full_profile(Provider::Proxmox) }, HostProfile { id: "aws1".into(), ..full_profile(Provider::Aws) }],
        };
        let targets = targets_of(&store);
        assert_eq!(targets.len(), 2);
        assert_eq!(targets[0], serde_json::json!({ "id": "pve1", "name": "Lab", "provider": "proxmox" }));
        assert_eq!(targets[1]["provider"], "aws");
    }

    #[test]
    fn the_default_host_is_a_saved_server_never_a_cloud() {
        let hosts =
            vec![HostProfile { id: "pve1".into(), ..full_profile(Provider::Proxmox) }, HostProfile { id: "aws1".into(), ..full_profile(Provider::Aws) }];
        assert_eq!(default_of(Store { default: Some("pve1".into()), hosts: hosts.clone() }).as_deref(), Some("pve1"));
        assert_eq!(default_of(Store { default: Some("aws1".into()), hosts: hosts.clone() }), None);
        assert_eq!(default_of(Store { default: Some("gone".into()), hosts: hosts.clone() }), None);
        assert_eq!(default_of(Store { default: None, hosts }), None);
    }

    #[test]
    fn mark_lab_writes_and_clears_the_marker() {
        let dir = std::env::temp_dir().join(format!("cyberctf-mark-{}", rand::random::<u32>()));
        std::fs::create_dir_all(&dir).unwrap();
        mark_lab(&dir, Some("pve1")).unwrap();
        assert_eq!(marked_host(&dir).as_deref(), Some("pve1"));
        mark_lab(&dir, None).unwrap();
        assert_eq!(marked_host(&dir), None);
        // Clearing an absent marker is fine.
        mark_lab(&dir, None).unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn esxi_env_contract() {
        let env = connection_env(&full_profile(Provider::VmwareEsxi), "s3cret");
        assert_eq!(get(&env, "CYBERCTF_REMOTE_PROVIDER"), Some("vmware_esxi"));
        assert_eq!(get(&env, "CYBERCTF_ESXI_HOSTNAME"), Some("10.0.0.5"));
        assert_eq!(get(&env, "CYBERCTF_ESXI_HOSTPORT"), Some("22"));
        assert_eq!(get(&env, "CYBERCTF_ESXI_PASSWORD"), Some("s3cret"));
        assert_eq!(get(&env, "CYBERCTF_ESXI_DISK_STORE"), Some("ssd1"));
        assert!(get(&env, "CYBERCTF_PROXMOX_ENDPOINT").is_none());
    }

    #[test]
    fn esxi_env_carries_isoloom_names_too() {
        let env = connection_env(&full_profile(Provider::VmwareEsxi), "s3cret");
        let names: Vec<&str> = env.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(
            names,
            [
                "CYBERCTF_REMOTE_PROVIDER",
                "CYBERCTF_REMOTE_HOST",
                "CYBERCTF_ESXI_HOSTNAME",
                "CYBERCTF_ESXI_HOSTPORT",
                "CYBERCTF_ESXI_USERNAME",
                "CYBERCTF_ESXI_PASSWORD",
                "CYBERCTF_ESXI_DISK_STORE",
                "CYBERCTF_ESXI_VIRTUAL_NETWORK",
                "ESXI_HOSTNAME",
                "ESXI_HOSTPORT",
                "ESXI_USERNAME",
                "ESXI_PASSWORD",
                "ESXI_DATASTORE",
                "ESXI_VIRTUAL_NETWORK",
            ]
        );
        assert_eq!(get(&env, "ESXI_DATASTORE"), Some("ssd1"));
        assert_eq!(get(&env, "ESXI_VIRTUAL_NETWORK"), Some("vmbr1"));
        // Optional settings that aren't set are left out under both names.
        let bare = connection_env(&profile(Provider::VmwareEsxi), "x");
        assert!(get(&bare, "CYBERCTF_ESXI_DISK_STORE").is_none() && get(&bare, "ESXI_DATASTORE").is_none());
    }

    #[test]
    fn proxmox_env_contract() {
        let mut p = full_profile(Provider::Proxmox);
        assert_eq!(get(&connection_env(&p, "x"), "CYBERCTF_PROXMOX_ENDPOINT"), Some("https://10.0.0.5:8006/api2/json"));
        p.host = "fd00::5".into();
        let env = connection_env(&p, "x");
        assert_eq!(get(&env, "CYBERCTF_PROXMOX_ENDPOINT"), Some("https://[fd00::5]:8006/api2/json"));
        assert_eq!(get(&env, "CYBERCTF_PROXMOX_USER_NAME"), Some("root@pam"));
        assert_eq!(get(&env, "CYBERCTF_PROXMOX_NODE"), Some("pve"));
        assert_eq!(get(&env, "CYBERCTF_PROXMOX_BRIDGE"), Some("vmbr1"));
    }

    #[test]
    fn cloud_hosts_have_no_vagrant_env_beyond_the_provider() {
        let env = connection_env(&profile(Provider::Aws), "x");
        assert_eq!(env.len(), 2);
        assert_eq!(get(&env, "CYBERCTF_REMOTE_PROVIDER"), Some("aws"));
    }

    #[test]
    fn marked_host_reads_the_trimmed_marker() {
        let dir = std::env::temp_dir().join(format!("cyberctf-marker-{}", rand::random::<u32>()));
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(marked_host(&dir), None);
        mark_lab(&dir, Some("ab12")).unwrap();
        assert_eq!(marked_host(&dir).as_deref(), Some("ab12"));
        std::fs::write(dir.join(HOST_MARKER), "ab12\n").unwrap();
        assert_eq!(marked_host(&dir).as_deref(), Some("ab12"));
        mark_lab(&dir, None).unwrap();
        assert_eq!(marked_host(&dir), None);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn aws_terraform_env_uses_keys_or_cli_profile() {
        let mut h = profile(Provider::Aws);
        h.host = "eu-west-3".into();
        h.username = "AKIA...".into();
        let env = terraform_env(&h, "secret");
        assert_eq!(get(&env, "AWS_ACCESS_KEY_ID"), Some("AKIA..."));
        assert_eq!(get(&env, "AWS_SECRET_ACCESS_KEY"), Some("secret"));
        assert_eq!(get(&env, "AWS_REGION"), Some("eu-west-3"));
        // CLI-credential mode passes only the region and profile, never keys.
        h.use_cli_creds = true;
        h.aws_profile = Some("work".into());
        let env = terraform_env(&h, "secret");
        assert!(get(&env, "AWS_ACCESS_KEY_ID").is_none());
        assert_eq!(get(&env, "AWS_REGION"), Some("eu-west-3"));
        assert_eq!(get(&env, "AWS_PROFILE"), Some("work"));
    }

    #[test]
    fn token_cloud_terraform_env_passes_the_token() {
        let mut h = profile(Provider::DigitalOcean);
        assert_eq!(get(&terraform_env(&h, "dop_v1_abc"), "DIGITALOCEAN_TOKEN"), Some("dop_v1_abc"));
        h.provider = Provider::Linode;
        assert_eq!(get(&terraform_env(&h, "lin_tok"), "LINODE_TOKEN"), Some("lin_tok"));
        // Azure's env only carries the subscription id; GCP carries nothing (ADC).
        h.provider = Provider::Azure;
        h.username = "sub-id".into();
        assert_eq!(get(&terraform_env(&h, ""), "ARM_SUBSCRIPTION_ID"), Some("sub-id"));
        h.provider = Provider::Gcp;
        assert!(terraform_env(&h, "").is_empty());
    }
}
