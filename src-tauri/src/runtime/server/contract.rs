//! What a lab is given to reach its server: the env a Vagrantfile reads, Terraform's env
//! and variables, and which server a lab directory is tied to.

use std::path::Path;

use crate::AppHandle;

use super::HOST_MARKER;
use super::profile::{DEFAULT_AUTO_STOP_HOURS, HostProfile};
use super::secrets::{get_secret, host_secret};
use super::store::{Store, load, load_host, saved_host};
use crate::error::Result;
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
            // The launcher's names, then the ones Isoloom's Vagrantfiles read (same values; the
            // datastore is named differently).
            for (prefix, datastore) in [("CYBERCTF_ESXI_", "DISK_STORE"), ("ESXI_", "DATASTORE")] {
                set(&format!("{prefix}HOSTNAME"), h.host.clone());
                set(&format!("{prefix}HOSTPORT"), h.port.to_string());
                set(&format!("{prefix}USERNAME"), h.username.clone());
                set(&format!("{prefix}PASSWORD"), password.into());
                if let Some(v) = &h.datastore {
                    set(&format!("{prefix}{datastore}"), v.clone());
                }
                if let Some(v) = &h.network {
                    set(&format!("{prefix}VIRTUAL_NETWORK"), v.clone());
                }
            }
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

/// Raw environment for Terraform (credentials the provider reads itself, never variables).
pub fn terraform_env(h: &HostProfile, password: &str) -> Vec<(String, String)> {
    let pair = |k: &str, v: &str| (k.to_string(), v.to_string());
    match h.provider {
        // CLI credentials: pass the region (and the chosen profile); Terraform reads the AWS
        // CLI's chain (works with a local terraform, which has the host's ~/.aws).
        Provider::Aws if h.use_cli_creds => {
            let mut env = vec![pair("AWS_REGION", &h.host)];
            if let Some(p) = &h.aws_profile {
                env.push(pair("AWS_PROFILE", p));
            }
            env
        }
        Provider::Aws => vec![pair("AWS_ACCESS_KEY_ID", &h.username), pair("AWS_SECRET_ACCESS_KEY", password), pair("AWS_REGION", &h.host)],
        // azurerm uses the Azure CLI's auth (az login); it only needs the subscription id.
        Provider::Azure => vec![pair("ARM_SUBSCRIPTION_ID", &h.username)],
        // The google provider uses gcloud's ADC; the project comes as a variable (see `gcp`).
        Provider::Gcp => Vec::new(),
        // The digitalocean provider reads DIGITALOCEAN_TOKEN (the stored API token).
        Provider::DigitalOcean => vec![pair("DIGITALOCEAN_TOKEN", password)],
        // The linode provider reads LINODE_TOKEN (the stored API token).
        Provider::Linode => vec![pair("LINODE_TOKEN", password)],
        _ => Vec::new(),
    }
}

/// Terraform variables for a host (the variables of Isoloom's modules).
pub fn terraform_vars(h: &HostProfile, password: &str) -> Vec<(String, String)> {
    if h.provider.is_cloud() { cloud_vars(h) } else { proxmox_vars(h, password) }
}

/// A cloud module's variables: region, auto-stop, size, and the account's own placement.
fn cloud_vars(h: &HostProfile) -> Vec<(String, String)> {
    let hours = h.auto_stop_hours.unwrap_or(DEFAULT_AUTO_STOP_HOURS);
    // `auto_stop_hours` is the launcher's own (expiry, reaper); Isoloom's modules take minutes.
    let mut vars = vec![
        ("region".to_string(), h.host.clone()),
        ("auto_stop_hours".to_string(), hours.to_string()),
        ("auto_stop_minutes".to_string(), (hours * 60).to_string()),
    ];
    // The size, under each module's own variable name.
    if let Some(t) = &h.datastore {
        let name = match h.provider {
            Provider::Azure | Provider::DigitalOcean => "size",
            Provider::Gcp => "machine_type",
            Provider::Linode => "type",
            _ => "instance_type",
        };
        vars.push((name.into(), t.clone()));
    }
    // GCP: the account's labs project (see `gcp`), else a project per lab from the billing
    // account (the module creates and deletes it).
    if h.provider == Provider::Gcp {
        match &h.gcp_project {
            Some(p) => vars.push(("project".into(), p.clone())),
            None => {
                vars.push(("billing_account".into(), h.username.clone()));
                if let Some(org) = &h.node {
                    vars.push(("org_id".into(), org.clone()));
                }
            }
        }
    }
    // OCI deploys into a compartment (the tenancy root works).
    if h.provider == Provider::Oci {
        vars.push(("compartment_id".into(), h.username.clone()));
    }
    vars
}

/// Isoloom's Proxmox module variables: the API connection, then node / storage / bridge.
fn proxmox_vars(h: &HostProfile, password: &str) -> Vec<(String, String)> {
    let mut vars = vec![
        ("proxmox_endpoint", proxmox::terraform_endpoint(h)),
        ("proxmox_username", proxmox::token_user(&h.username).to_string()),
        ("proxmox_insecure", h.insecure_tls.to_string()),
        // Snippets go over SSH to the address the player entered.
        ("proxmox_ssh_address", h.host.clone()),
    ];
    // An API token replaces the password for the API; the snippet upload then uses the
    // launcher's SSH key (added at apply time, see terraform).
    if proxmox::is_token(&h.username) {
        vars.push(("proxmox_api_token", proxmox::terraform_token(&h.username, password)));
        vars.push(("proxmox_ssh_username", proxmox::ssh_user(&h.username).to_string()));
    } else {
        vars.push(("proxmox_password", password.to_string()));
    }
    if let Some(v) = &h.node {
        vars.push(("node", v.clone()));
    }
    if let Some(v) = &h.datastore {
        vars.push(("datastore", v.clone()));
    }
    if let Some(v) = &h.network {
        vars.push(("uplink_bridge", v.clone()));
    }
    vars.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}

// --- resolving a host for a launch ------------------------------------------

/// A host resolved for a launch: its provider, the Vagrant env (Vagrantfile contract) and
/// the Terraform variables (Terraform modules).
pub struct Connection {
    pub provider: Provider,
    pub name: String,
    pub env: Vec<(String, String)>,
    pub tf_vars: Vec<(String, String)>,
    /// Raw environment for Terraform (cloud credentials the provider reads itself).
    pub tf_env: Vec<(String, String)>,
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
        tf_vars: terraform_vars(host, password),
        tf_env: terraform_env(host, password),
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
    if !proxmox::bridge_is_internal(&host, &secret).await {
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
    fn aws_keys_vars_carry_no_proxmox_settings() {
        let h = HostProfile {
            provider: Provider::Aws,
            host: "eu-west-3".into(),
            username: "AKIAIOSFODNN7EXAMPLE".into(),
            datastore: Some("t3.small".into()),
            ..full_profile(Provider::Proxmox)
        };
        assert_eq!(get(&terraform_env(&h, "sec"), "AWS_SECRET_ACCESS_KEY"), Some("sec"));
        let vars = terraform_vars(&h, "sec");
        assert_eq!(get(&vars, "region"), Some("eu-west-3"));
        assert!(get(&vars, "proxmox_password").is_none());
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

    #[test]
    fn cloud_terraform_vars_carry_region_and_auto_stop_minutes() {
        let mut h = profile(Provider::Aws);
        h.host = "eu-west-3".into();
        h.auto_stop_hours = Some(3);
        h.datastore = Some("t3.small".into());
        let vars = terraform_vars(&h, "sec");
        assert_eq!(get(&vars, "region"), Some("eu-west-3"));
        assert_eq!(get(&vars, "auto_stop_hours"), Some("3"));
        assert_eq!(get(&vars, "auto_stop_minutes"), Some("180"));
        // AWS size var is instance_type.
        assert_eq!(get(&vars, "instance_type"), Some("t3.small"));
        // No auto_stop_hours set falls back to the default.
        h.auto_stop_hours = None;
        let vars = terraform_vars(&h, "sec");
        assert_eq!(get(&vars, "auto_stop_hours"), Some(DEFAULT_AUTO_STOP_HOURS.to_string().as_str()));
    }

    #[test]
    fn cloud_size_var_name_differs_per_provider() {
        let size = |p: Provider, var: &str| {
            let mut h = profile(p);
            h.datastore = Some("sz".into());
            h.host = match p {
                Provider::Azure => "westeurope".into(),
                Provider::Gcp => "europe-west1".into(),
                Provider::DigitalOcean => "fra1".into(),
                Provider::Linode => "eu-central".into(),
                _ => "eu-west-3".into(),
            };
            assert_eq!(get(&terraform_vars(&h, ""), var), Some("sz"), "{p:?}");
        };
        size(Provider::Azure, "size");
        size(Provider::DigitalOcean, "size");
        size(Provider::Gcp, "machine_type");
        size(Provider::Linode, "type");
        size(Provider::Aws, "instance_type");
    }

    #[test]
    fn gcp_vars_prefer_existing_project_else_billing_account() {
        let mut h = profile(Provider::Gcp);
        h.host = "europe-west1".into();
        h.username = "0X0X0X-0X0X0X-0X0X0X".into();
        h.node = Some("123456789012".into());
        // No project yet: pass billing account and org.
        let vars = terraform_vars(&h, "");
        assert_eq!(get(&vars, "billing_account"), Some("0X0X0X-0X0X0X-0X0X0X"));
        assert_eq!(get(&vars, "org_id"), Some("123456789012"));
        assert!(get(&vars, "project").is_none());
        // An established project is reused instead.
        h.gcp_project = Some("cyberctf-labs-xyz".into());
        let vars = terraform_vars(&h, "");
        assert_eq!(get(&vars, "project"), Some("cyberctf-labs-xyz"));
        assert!(get(&vars, "billing_account").is_none());
    }

    #[test]
    fn oci_vars_carry_the_compartment() {
        let mut h = profile(Provider::Oci);
        h.host = "eu-frankfurt-1".into();
        h.username = "ocid1.compartment.oc1..aaaa".into();
        assert_eq!(get(&terraform_vars(&h, ""), "compartment_id"), Some("ocid1.compartment.oc1..aaaa"));
    }

    #[test]
    fn proxmox_password_vars_build_the_endpoint_and_connection() {
        let mut h = profile(Provider::Proxmox);
        h.username = "root@pam".into();
        h.node = Some("pve".into());
        let vars = terraform_vars(&h, "pw");
        assert_eq!(get(&vars, "proxmox_endpoint"), Some("https://10.0.0.5:8006/"));
        assert_eq!(get(&vars, "proxmox_username"), Some("root@pam"));
        assert_eq!(get(&vars, "proxmox_password"), Some("pw"));
        assert_eq!(get(&vars, "node"), Some("pve"));
        // A password setup carries no api token var.
        assert!(get(&vars, "proxmox_api_token").is_none());
    }

    #[test]
    fn proxmox_token_vars_use_api_token_and_ssh_user() {
        let mut h = profile(Provider::Proxmox);
        h.username = "root@pam!cyberctf".into();
        let vars = terraform_vars(&h, "sec");
        assert_eq!(get(&vars, "proxmox_username"), Some("root@pam"));
        assert_eq!(get(&vars, "proxmox_api_token"), Some("root@pam!cyberctf=sec"));
        assert_eq!(get(&vars, "proxmox_ssh_username"), Some("root"));
        assert!(get(&vars, "proxmox_password").is_none());
    }
}
