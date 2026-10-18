//! The launcher's hosts and cloud accounts in the terms of Isoloom's Terraform modules (their
//! variables, docs/cloud-modules.md, and the Proxmox module's) and of its Vagrantfiles (the ESXi
//! plugin's environment). Isoloom's modules keep their own conventions; the mapping from the
//! launcher's providers, profiles and credentials to them lives here.

use crate::runtime::providers::Provider;
use crate::runtime::proxmox;
use crate::runtime::server::{DEFAULT_AUTO_STOP_HOURS, HostProfile, size_var};

/// The environment Isoloom's Vagrantfiles read to reach an ESXi host (the `vagrant-vmware-esxi`
/// plugin's settings), from the host's profile and password.
pub fn esxi_env(h: &HostProfile, password: &str) -> Vec<(String, String)> {
    let mut env = vec![
        ("ESXI_HOSTNAME".to_string(), h.host.clone()),
        ("ESXI_HOSTPORT".to_string(), h.port.to_string()),
        ("ESXI_USERNAME".to_string(), h.username.clone()),
        ("ESXI_PASSWORD".to_string(), password.to_string()),
    ];
    if let Some(v) = &h.datastore {
        env.push(("ESXI_DATASTORE".into(), v.clone()));
    }
    if let Some(v) = &h.network {
        env.push(("ESXI_VIRTUAL_NETWORK".into(), v.clone()));
    }
    env
}

/// The variables of a lab's Isoloom module on host `h` (a cloud account or a Proxmox server),
/// with its secret: where and how big, and how to reach it.
pub fn host_vars(h: &HostProfile, password: &str) -> Vec<(String, String)> {
    if h.provider.is_cloud() { cloud_vars(h) } else { proxmox_vars(h, password) }
}

/// A cloud module's variables: region, auto-stop, size, and the account's own placement.
fn cloud_vars(h: &HostProfile) -> Vec<(String, String)> {
    let hours = h.auto_stop_hours.unwrap_or(DEFAULT_AUTO_STOP_HOURS);
    // `auto_stop_hours` is the launcher's own (expiry, reaper); Isoloom's modules take minutes.
    let mut vars = vec![
        ("region".to_string(), h.region().unwrap_or_default().to_string()),
        ("auto_stop_hours".to_string(), hours.to_string()),
        // Saturating: the form caps the hours, but the profile file can hold anything.
        ("auto_stop_minutes".to_string(), hours.saturating_mul(60).to_string()),
    ];
    // The size, under each module's own variable name.
    if let (Some(size), Some(name)) = (h.size(), size_var(h.provider)) {
        vars.push((name.into(), size.to_string()));
    }
    // GCP: the account's labs project (see `gcp`), else a project per lab from the billing
    // account (the module creates and deletes it).
    if h.provider == Provider::Gcp {
        match &h.gcp_project {
            Some(p) => vars.push(("project".into(), p.clone())),
            None => {
                vars.push(("billing_account".into(), h.cloud_account().unwrap_or_default().to_string()));
                if let Some(org) = h.gcp_org() {
                    vars.push(("org_id".into(), org.to_string()));
                }
            }
        }
    }
    // OCI deploys into a compartment (the tenancy root works).
    if h.provider == Provider::Oci {
        vars.push(("compartment_id".into(), h.cloud_account().unwrap_or_default().to_string()));
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::server::profile_tests::{get, profile as full_profile};

    /// A bare profile: no storage, network or node set.
    fn profile(provider: Provider) -> HostProfile {
        HostProfile { port: 8006, username: "root".into(), datastore: None, network: None, node: None, ..full_profile(provider) }
    }

    #[test]
    fn esxi_env_is_the_plugins() {
        let env = esxi_env(&full_profile(Provider::VmwareEsxi), "s3cret");
        let names: Vec<&str> = env.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(names, ["ESXI_HOSTNAME", "ESXI_HOSTPORT", "ESXI_USERNAME", "ESXI_PASSWORD", "ESXI_DATASTORE", "ESXI_VIRTUAL_NETWORK"]);
        assert_eq!(get(&env, "ESXI_DATASTORE"), Some("ssd1"));
        assert!(get(&esxi_env(&profile(Provider::VmwareEsxi), "x"), "ESXI_DATASTORE").is_none());
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
        let vars = host_vars(&h, "sec");
        assert_eq!(get(&vars, "region"), Some("eu-west-3"));
        assert!(get(&vars, "proxmox_password").is_none());
    }

    #[test]
    fn a_zero_auto_stop_is_zero_minutes_and_no_size_leaves_the_modules_default() {
        let mut h = profile(Provider::Aws);
        h.host = "eu-west-3".into();
        h.auto_stop_hours = Some(0);
        let vars = host_vars(&h, "");
        assert_eq!((get(&vars, "auto_stop_hours"), get(&vars, "auto_stop_minutes")), (Some("0"), Some("0")));
        assert!(get(&vars, "instance_type").is_none());
    }

    #[test]
    fn an_auto_stop_beyond_the_u32_range_saturates_instead_of_wrapping() {
        // The form caps the value at 72, but the profile is read back from a file an older
        // version or a hand edit can hold anything in: it panicked (debug) or wrapped (release).
        let mut h = profile(Provider::Aws);
        h.host = "eu-west-3".into();
        h.auto_stop_hours = Some(u32::MAX / 60 + 1);
        let vars = host_vars(&h, "");
        assert_eq!(get(&vars, "auto_stop_minutes"), Some(u32::MAX.to_string().as_str()));
    }

    #[test]
    fn cloud_terraform_vars_carry_region_and_auto_stop_minutes() {
        let mut h = profile(Provider::Aws);
        h.host = "eu-west-3".into();
        h.auto_stop_hours = Some(3);
        h.datastore = Some("t3.small".into());
        let vars = host_vars(&h, "sec");
        assert_eq!(get(&vars, "region"), Some("eu-west-3"));
        assert_eq!(get(&vars, "auto_stop_hours"), Some("3"));
        assert_eq!(get(&vars, "auto_stop_minutes"), Some("180"));
        // AWS size var is instance_type.
        assert_eq!(get(&vars, "instance_type"), Some("t3.small"));
        // No auto_stop_hours set falls back to the default.
        h.auto_stop_hours = None;
        let vars = host_vars(&h, "sec");
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
            assert_eq!(get(&host_vars(&h, ""), var), Some("sz"), "{p:?}");
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
        let vars = host_vars(&h, "");
        assert_eq!(get(&vars, "billing_account"), Some("0X0X0X-0X0X0X-0X0X0X"));
        assert_eq!(get(&vars, "org_id"), Some("123456789012"));
        assert!(get(&vars, "project").is_none());
        // An established project is reused instead.
        h.gcp_project = Some("cyberctf-labs-xyz".into());
        let vars = host_vars(&h, "");
        assert_eq!(get(&vars, "project"), Some("cyberctf-labs-xyz"));
        assert!(get(&vars, "billing_account").is_none());
    }

    #[test]
    fn oci_vars_carry_the_compartment() {
        let mut h = profile(Provider::Oci);
        h.host = "eu-frankfurt-1".into();
        h.username = "ocid1.compartment.oc1..aaaa".into();
        assert_eq!(get(&host_vars(&h, ""), "compartment_id"), Some("ocid1.compartment.oc1..aaaa"));
    }

    #[test]
    fn proxmox_password_vars_build_the_endpoint_and_connection() {
        let mut h = profile(Provider::Proxmox);
        h.username = "root@pam".into();
        h.node = Some("pve".into());
        let vars = host_vars(&h, "pw");
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
        let vars = host_vars(&h, "sec");
        assert_eq!(get(&vars, "proxmox_username"), Some("root@pam"));
        assert_eq!(get(&vars, "proxmox_api_token"), Some("root@pam!cyberctf=sec"));
        assert_eq!(get(&vars, "proxmox_ssh_username"), Some("root"));
        assert!(get(&vars, "proxmox_password").is_none());
    }
}
