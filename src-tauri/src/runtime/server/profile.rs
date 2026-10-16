//! A saved server host or cloud account (non-secret settings only) and the per-provider
//! defaults that go with it.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::kind::{HostField, KINDS, Secret, kind};
use crate::runtime::providers::Provider;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "ServerHost")]
pub struct HostProfile {
    pub id: String,
    /// Display name, e.g. "Garage Proxmox".
    pub name: String,
    /// `vmware_esxi`, `proxmox` or a cloud (`aws`, `azure`, ...).
    pub provider: Provider,
    /// Hostname or IP, no scheme. Clouds: the region.
    pub host: String,
    /// ESXi: SSH port (22). Proxmox: API port (8006).
    pub port: u16,
    /// ESXi: e.g. `root`. Proxmox: user with realm, e.g. `root@pam`. Clouds: the account
    /// identifier (AWS access key id, Azure subscription, GCP billing account, OCI compartment).
    pub username: String,
    /// ESXi datastore / Proxmox storage for lab disks. Clouds: the instance size.
    #[serde(default)]
    pub datastore: Option<String>,
    /// ESXi port group / Proxmox bridge for lab NICs.
    #[serde(default)]
    pub network: Option<String>,
    /// Proxmox node name. GCP: the optional organization id.
    #[serde(default)]
    pub node: Option<String>,
    /// Proxmox: accept the API's self-signed certificate (the Proxmox default).
    #[serde(default)]
    pub insecure_tls: bool,
    /// Clouds: terminate a lab's instance this many hours after it starts (0 = never).
    #[serde(default)]
    pub auto_stop_hours: Option<u32>,
    /// AWS: use the AWS CLI's own credentials (the default chain, from `aws configure`)
    /// instead of access keys stored by the launcher. No secret is kept in the keychain.
    #[serde(default)]
    pub use_cli_creds: bool,
    /// AWS CLI profile to use with `use_cli_creds` (None = the default profile).
    #[serde(default)]
    pub aws_profile: Option<String>,
    /// AWS: a monthly spend limit in USD; new cloud labs are blocked once this month's
    /// cost passes it. None / 0 = no limit.
    #[serde(default)]
    pub monthly_limit: Option<f64>,
    /// GCP: the project every lab of this account runs in (created once, see `gcp`).
    #[serde(default)]
    pub gcp_project: Option<String>,
}

impl HostProfile {
    /// Whether the keychain holds a secret for this host. AWS in CLI mode, Azure, GCP and OCI
    /// sign in through their own CLI / config, so the launcher keeps nothing for them.
    pub(super) fn keeps_secret(&self) -> bool {
        !self.use_cli_creds && kind(self.provider).is_some_and(|k| k.secret != Secret::None)
    }

    /// A cloud account's region (its `host`); None for a server, whose `host` is an address.
    pub fn region(&self) -> Option<&str> {
        kind(self.provider).filter(|k| k.host == HostField::Region).map(|_| self.host.as_str())
    }

    /// The account a cloud creates labs in, as its `username` names it: the Azure subscription,
    /// the GCP billing account, the OCI compartment. None for the others, whose `username` is a
    /// login or a key id.
    pub fn cloud_account(&self) -> Option<&str> {
        kind(self.provider).filter(|k| k.account.is_placement()).map(|_| self.username.as_str())
    }

    /// A cloud instance's size (its `datastore`); None on a server, where it is the storage.
    pub fn size(&self) -> Option<&str> {
        self.datastore.as_deref().filter(|_| self.provider.is_cloud())
    }

    /// A GCP account's organization (its `node`); None for the others.
    pub fn gcp_org(&self) -> Option<&str> {
        self.node.as_deref().filter(|_| self.provider == Provider::Gcp)
    }

    /// What says where this host's labs are (its provider, address or region, port, Proxmox node,
    /// the account labs are created in, an AWS CLI profile), never a renewable credential: while
    /// labs are deployed on it, a stop must still find them where they were created.
    pub(super) fn placement(&self) -> Vec<(&'static str, String)> {
        let mut fields = vec![
            ("provider", self.provider.id().to_string()),
            ("address or region", self.host.clone()),
            ("port", self.port.to_string()),
            ("node or organization", self.node.clone().unwrap_or_default()),
        ];
        fields.extend(self.cloud_account().map(|a| ("account", a.to_string())));
        // AWS through the CLI: the profile picks the account (another profile is likely another
        // account). Stored keys can't be told apart offline: renewing them must stay possible.
        if self.use_cli_creds {
            fields.push(("AWS CLI profile", self.aws_profile.clone().unwrap_or_default()));
        }
        fields
    }
}

/// Cloud labs stop themselves after this long unless the account says otherwise.
pub const DEFAULT_AUTO_STOP_HOURS: u32 = 4;

pub(super) fn default_port(provider: Provider) -> u16 {
    kind(provider).map_or(22, |k| k.default_port)
}

/// The Terraform target (state folder, Isoloom module) for a provider, if it uses Terraform.
pub fn terraform_target(provider: Provider) -> Option<&'static str> {
    kind(provider).and_then(|k| k.terraform)
}

/// The provider whose Terraform target (a state folder's name) is `target`.
pub fn terraform_provider(target: &str) -> Option<Provider> {
    KINDS.iter().find(|k| k.terraform == Some(target)).map(|k| k.provider)
}

/// `bytes` random bytes as lowercase hex (host ids, GCP project suffixes).
pub(super) fn random_hex(bytes: usize) -> String {
    (0..bytes).map(|_| format!("{:02x}", rand::random::<u8>())).collect()
}

/// A new host id: 16 hex characters.
pub(super) fn new_id() -> String {
    random_hex(8)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A complete profile for tests: a LAN address, the provider's default port, a username of
    /// the provider's shape and the optional fields set.
    pub fn profile(provider: Provider) -> HostProfile {
        HostProfile {
            gcp_project: None,
            id: "ab12".into(),
            name: "Lab".into(),
            provider,
            host: "10.0.0.5".into(),
            port: default_port(provider),
            username: if provider == Provider::Proxmox { "root@pam".into() } else { "root".into() },
            datastore: Some("ssd1".into()),
            network: Some("vmbr1".into()),
            node: Some("pve".into()),
            insecure_tls: false,
            auto_stop_hours: None,
            use_cli_creds: false,
            aws_profile: None,
            monthly_limit: None,
        }
    }

    pub fn get<'a>(env: &'a [(String, String)], k: &str) -> Option<&'a str> {
        env.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str())
    }

    #[test]
    fn default_ports_per_provider() {
        assert_eq!(default_port(Provider::Proxmox), 8006);
        assert_eq!(default_port(Provider::VmwareEsxi), 22);
        for cloud in [Provider::Aws, Provider::Azure, Provider::Gcp, Provider::DigitalOcean, Provider::Linode, Provider::Oci] {
            assert_eq!(default_port(cloud), 443, "{cloud:?}");
        }
    }

    #[test]
    fn terraform_target_only_for_terraform_providers() {
        assert_eq!(terraform_target(Provider::Proxmox), Some("proxmox"));
        assert_eq!(terraform_target(Provider::Aws), Some("aws"));
        assert_eq!(terraform_target(Provider::Oci), Some("oci"));
        assert_eq!(terraform_provider("digitalocean"), Some(Provider::DigitalOcean));
        assert_eq!(terraform_provider("proxmox"), Some(Provider::Proxmox));
        assert_eq!((terraform_provider("vmware_esxi"), terraform_provider("lab")), (None, None), "ESXi runs no Terraform");
        for cloud in [Provider::Azure, Provider::Gcp, Provider::DigitalOcean, Provider::Linode] {
            assert_eq!(terraform_target(cloud), Some(cloud.id()), "{cloud:?}");
        }
        // ESXi and local hypervisors run through Vagrant, not Terraform.
        assert_eq!(terraform_target(Provider::VmwareEsxi), None);
        assert_eq!(terraform_target(Provider::Virtualbox), None);
    }

    #[test]
    fn only_key_and_token_hosts_keep_a_secret() {
        for p in [Provider::VmwareEsxi, Provider::Proxmox, Provider::Aws, Provider::DigitalOcean, Provider::Linode] {
            assert!(profile(p).keeps_secret(), "{p:?}");
        }
        for p in [Provider::Azure, Provider::Gcp, Provider::Oci] {
            assert!(!profile(p).keeps_secret(), "{p:?}");
        }
        let cli = HostProfile { use_cli_creds: true, ..profile(Provider::Aws) };
        assert!(!cli.keeps_secret());
    }

    #[test]
    fn ids_are_hex_of_the_asked_length() {
        let id = new_id();
        assert_eq!(id.len(), 16);
        assert!(id.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
        assert_eq!(random_hex(3).len(), 6);
    }

    #[test]
    fn host_profile_json_round_trips() {
        let p = profile(Provider::Proxmox);
        let json = serde_json::to_string(&p).unwrap();
        let back: HostProfile = serde_json::from_str(&json).unwrap();
        assert_eq!(back, p);
        // Optional fields default when absent (older server.json files).
        let minimal = r#"{"id":"ab12","name":"L","provider":"proxmox","host":"10.0.0.5","port":8006,"username":"root@pam"}"#;
        let m: HostProfile = serde_json::from_str(minimal).unwrap();
        assert_eq!(m.datastore, None);
        assert!(!m.insecure_tls);
        assert_eq!(m.monthly_limit, None);
    }
}
