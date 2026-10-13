//! What the setup form submits, and turning it into a `HostProfile`: every field is checked
//! against its provider's shape (region ids, account ids, tokens) before anything is saved.

use serde::Deserialize;
use ts_rs::TS;

use super::profile::{DEFAULT_AUTO_STOP_HOURS, HostProfile, default_port};
use crate::error::{Error, Result};
use crate::runtime::providers::Provider;

/// What the UI submits. `id` is None for a new host; `password` None keeps the stored one.
#[derive(Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "ServerHostInput")]
pub struct HostInput {
    pub(super) id: Option<String>,
    name: String,
    pub(super) provider: Provider,
    host: String,
    port: Option<u16>,
    username: String,
    datastore: Option<String>,
    network: Option<String>,
    node: Option<String>,
    insecure_tls: Option<bool>,
    auto_stop_hours: Option<u32>,
    pub(super) password: Option<String>,
    #[serde(default)]
    #[ts(optional = nullable)]
    use_cli_creds: Option<bool>,
    #[serde(default)]
    #[ts(optional = nullable)]
    aws_profile: Option<String>,
    #[serde(default)]
    #[ts(optional = nullable)]
    monthly_limit: Option<f64>,
}

/// The connection fields of an input, checked: what `HostInput::into_profile` builds on.
pub(super) struct Checked {
    pub host: String,
    pub username: String,
    /// AWS through the AWS CLI's own credentials (no stored keys).
    pub use_cli: bool,
}

fn require(ok: bool, message: &str) -> Result<()> {
    if ok { Ok(()) } else { Err(Error::Invalid(message.into())) }
}

impl HostInput {
    /// Checks the connection fields against the provider (address, account, credentials shape).
    pub(super) fn check(&self) -> Result<Checked> {
        require(self.provider.is_remote(), "a host must be ESXi, Proxmox, AWS, Azure, GCP, DigitalOcean, Linode or OCI")?;
        let host = clean(&self.host, "host", 253)?;
        // AWS can connect with the AWS CLI's own credentials instead of stored keys.
        let use_cli = self.provider == Provider::Aws && self.use_cli_creds.unwrap_or(false);
        // DigitalOcean and Linode authenticate with just an API token (stored as the secret); no username.
        let token_only = matches!(self.provider, Provider::DigitalOcean | Provider::Linode);
        let username = if use_cli || token_only { String::new() } else { clean(&self.username, "username", 128)? };
        // A new token must look like one; None or empty keeps the stored one.
        let token_ok = || self.password.as_deref().filter(|p| !p.is_empty()).is_none_or(valid_do_token);
        match self.provider {
            Provider::Aws => {
                require(valid_region(&host), "region must be an AWS region id, e.g. eu-west-3")?;
                if !use_cli {
                    require(
                        !username.starts_with("ASIA"),
                        "those are temporary credentials (ASIA...), which expire and need a session token. Switch to \"Connect with the AWS CLI\" and sign in with `aws login` / `aws sso login` so the launcher always has fresh credentials.",
                    )?;
                    require(valid_access_key_id(&username), "access key id looks wrong; an IAM user's key starts with AKIA...")?;
                }
            }
            Provider::Azure => {
                require(valid_azure_location(&host), "region must be an Azure location, e.g. westeurope")?;
                require(valid_subscription(&username), "subscription must be a GUID (see `az account show`)")?;
            }
            Provider::Gcp => {
                require(valid_gcp_region(&host), "region must be a GCP region id, e.g. europe-west1")?;
                require(valid_billing_account(&username), "billing account must look like 0X0X0X-0X0X0X-0X0X0X (see `gcloud billing accounts list`)")?;
                require(valid_org_id(self.node.as_deref().unwrap_or_default()), "organization id must be all digits, or empty for a personal account")?;
            }
            Provider::DigitalOcean => {
                require(valid_do_region(&host), "region must be a DigitalOcean region slug, e.g. fra1 or nyc3")?;
                require(token_ok(), "that doesn't look like a DigitalOcean API token")?;
            }
            Provider::Linode => {
                require(valid_linode_region(&host), "region must be a Linode region slug, e.g. eu-central or us-east")?;
                require(token_ok(), "that doesn't look like a Linode API token")?;
            }
            Provider::Oci => {
                require(valid_oci_region(&host), "region must be an OCI region id, e.g. eu-frankfurt-1")?;
                require(valid_ocid(&username), "compartment must be an OCID (ocid1.compartment... or the tenancy OCID)")?;
            }
            _ => require(valid_host(&host), "host must be a hostname or IP address, without https:// or a path")?,
        }
        require(self.provider != Provider::Proxmox || username.contains('@'), "Proxmox users include a realm, e.g. root@pam")?;
        Ok(Checked { host, username, use_cli })
    }

    /// The profile to save under `id`. `gcp_project` is the labs project kept from the previous
    /// save, if any.
    pub(super) fn into_profile(self, id: String, checked: Checked, gcp_project: Option<String>) -> Result<HostProfile> {
        let Checked { host, username, use_cli } = checked;
        let provider = self.provider;
        Ok(HostProfile {
            gcp_project,
            id,
            name: clean(&self.name, "name", 64)?,
            provider,
            host,
            port: self.port.filter(|p| *p > 0).unwrap_or(default_port(provider)),
            username,
            datastore: clean_opt(self.datastore, "datastore")?,
            network: clean_opt(self.network, "network")?,
            // Proxmox uses `node`; GCP reuses it for the optional organization id.
            node: if matches!(provider, Provider::Proxmox | Provider::Gcp) { clean_opt(self.node, "node")? } else { None },
            insecure_tls: provider == Provider::Proxmox && self.insecure_tls.unwrap_or(false),
            auto_stop_hours: if provider.is_cloud() {
                match self.auto_stop_hours.unwrap_or(DEFAULT_AUTO_STOP_HOURS) {
                    h if h <= 72 => Some(h),
                    _ => return Err(Error::Invalid("auto-stop must be between 0 and 72 hours".into())),
                }
            } else {
                None
            },
            use_cli_creds: use_cli,
            aws_profile: if use_cli { clean_opt(self.aws_profile, "profile")? } else { None },
            // Budget enforcement is AWS-only (the other clouds have no cost read yet), so don't
            // persist a limit that would silently do nothing on Azure/GCP.
            monthly_limit: if provider == Provider::Aws { self.monthly_limit.filter(|v| *v > 0.0) } else { None },
        })
    }
}

// --- field shapes ---------------------------------------------------------

pub(super) fn clean(value: &str, field: &str, max: usize) -> Result<String> {
    let v = value.trim();
    if v.is_empty() || v.len() > max || v.chars().any(char::is_control) {
        return Err(Error::Invalid(format!("invalid {field}")));
    }
    Ok(v.to_string())
}

pub(super) fn clean_opt(value: Option<String>, field: &str) -> Result<Option<String>> {
    match value.as_deref().map(str::trim) {
        None | Some("") => Ok(None),
        Some(v) => clean(v, field, 128).map(Some),
    }
}

/// A bare hostname or IP (v4 or v6), never a URL: it is interpolated into an endpoint. A colon
/// is only an IPv6 address's: `pve.lan:8006` pasted from an address bar was bracketed as one
/// (`https://[pve.lan:8006]:8006/`) and failed with no hint about the port.
fn valid_host(host: &str) -> bool {
    let chars_ok = host.chars().all(|c| c.is_ascii_alphanumeric() || ".-:".contains(c));
    let colon_is_ipv6 = !host.contains(':') || host.parse::<std::net::Ipv6Addr>().is_ok();
    !host.is_empty() && host.len() <= 253 && chars_ok && colon_is_ipv6 && !host.starts_with('-')
}

/// A host id as `new_id` makes it (any case).
pub(super) fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 32 && id.chars().all(|c| c.is_ascii_hexdigit())
}

/// An AWS region id, e.g. eu-west-3, us-gov-west-1.
fn valid_region(region: &str) -> bool {
    let parts: Vec<&str> = region.split('-').collect();
    parts.len() >= 3
        && parts[0].len() == 2
        && parts.iter().all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()))
        && parts.last().is_some_and(|p| p.chars().all(|c| c.is_ascii_digit()))
}

/// An Azure location id, e.g. "westeurope" (lowercase letters/digits, no spaces).
fn valid_azure_location(s: &str) -> bool {
    !s.is_empty() && s.len() <= 32 && s.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
}

/// An Azure subscription id (a GUID, 8-4-4-4-12 hex).
fn valid_subscription(s: &str) -> bool {
    let p: Vec<&str> = s.split('-').collect();
    p.len() == 5 && [8usize, 4, 4, 4, 12].iter().zip(&p).all(|(n, seg)| seg.len() == *n && seg.chars().all(|c| c.is_ascii_hexdigit()))
}

/// A GCP region id, e.g. "europe-west1", "us-central1" (lowercase letters/digits with a hyphen,
/// starting with a letter so it never reads as a flag).
fn valid_gcp_region(s: &str) -> bool {
    s.len() <= 32 && s.contains('-') && starts_lower_slug(s)
}

/// A GCP billing account id: three 6-char groups of uppercase hex, e.g. 0X0X0X-0X0X0X-0X0X0X.
fn valid_billing_account(s: &str) -> bool {
    let p: Vec<&str> = s.split('-').collect();
    p.len() == 3 && p.iter().all(|g| g.len() == 6 && g.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit()))
}

/// A GCP organization id: all digits (e.g. 123456789012). Empty = personal / no-org account.
fn valid_org_id(s: &str) -> bool {
    s.is_empty() || (s.len() <= 32 && s.chars().all(|c| c.is_ascii_digit()))
}

/// A DigitalOcean region slug, e.g. "fra1", "nyc3" (lowercase letters then digits).
fn valid_do_region(s: &str) -> bool {
    (3..=8).contains(&s.len()) && s.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
}

/// A DigitalOcean / Linode API token: the "dop_v1_..." form or a hex PAT. Loose length/charset check.
fn valid_do_token(s: &str) -> bool {
    (40..=200).contains(&s.len()) && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// A Linode region slug, e.g. "eu-central", "us-east", "ap-south" (lowercase letters/digits/hyphens).
fn valid_linode_region(s: &str) -> bool {
    (3..=20).contains(&s.len()) && starts_lower_slug(s)
}

/// An OCI region id, e.g. "eu-frankfurt-1", "us-ashburn-1" (lowercase letters/digits/hyphens).
fn valid_oci_region(s: &str) -> bool {
    (5..=24).contains(&s.len()) && starts_lower_slug(s)
}

/// Lowercase letters, digits and hyphens, starting with a letter.
fn starts_lower_slug(s: &str) -> bool {
    s.starts_with(|c: char| c.is_ascii_lowercase()) && s.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// An OCID (tenancy / compartment / ...): starts with "ocid1." then dot/hyphen-separated alnum.
fn valid_ocid(s: &str) -> bool {
    s.starts_with("ocid1.") && (20..=255).contains(&s.len()) && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_')
}

/// A long-term AWS access key id (AKIA...). Temporary ASIA... keys are refused separately in
/// `check`: they need a session token and expire, so the launcher steers them to CLI mode.
fn valid_access_key_id(id: &str) -> bool {
    (16..=128).contains(&id.len()) && id.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(v: serde_json::Value) -> HostInput {
        serde_json::from_value(v).unwrap()
    }

    fn error(i: &HostInput) -> String {
        i.check().err().map(|e| e.to_string()).unwrap_or_default()
    }

    #[test]
    fn check_validates_azure_and_oci_accounts() {
        let azure = |host: &str, sub: &str| input(serde_json::json!({ "name": "A", "provider": "azure", "host": host, "username": sub }));
        assert!(azure("westeurope", "00000000-0000-0000-0000-000000000000").check().is_ok());
        assert_eq!(error(&azure("West Europe", "00000000-0000-0000-0000-000000000000")), "region must be an Azure location, e.g. westeurope");
        assert_eq!(error(&azure("westeurope", "not-a-guid")), "subscription must be a GUID (see `az account show`)");
        let oci = |host: &str, ocid: &str| input(serde_json::json!({ "name": "O", "provider": "oci", "host": host, "username": ocid }));
        assert!(oci("eu-frankfurt-1", "ocid1.tenancy.oc1..aaaaaaaabbbbbbbb").check().is_ok());
        assert_eq!(error(&oci("EU_Frankfurt", "ocid1.tenancy.oc1..aaaaaaaabbbbbbbb")), "region must be an OCI region id, e.g. eu-frankfurt-1");
        assert_eq!(error(&oci("eu-frankfurt-1", "tenancy")), "compartment must be an OCID (ocid1.compartment... or the tenancy OCID)");
    }

    #[test]
    fn aws_regions_and_keys() {
        for good in ["eu-west-3", "us-east-1", "us-gov-west-1", "ap-southeast-2"] {
            assert!(valid_region(good), "{good}");
        }
        for bad in ["eu-west", "EU-west-3", "eu_west_3", "https://eu-west-3", "eu-west-3a"] {
            assert!(!valid_region(bad), "{bad}");
        }
        assert!(valid_access_key_id("AKIAIOSFODNN7EXAMPLE"));
        assert!(!valid_access_key_id("akiaiosfodnn7example"));
    }

    #[test]
    fn hosts_are_bare_names_or_ips() {
        for good in ["pve.lan", "10.0.0.5", "fd00::5", "esxi-01"] {
            assert!(valid_host(good), "{good}");
        }
        for bad in ["https://pve", "pve/api", "a b", "-x", "pve;rm", ""] {
            assert!(!valid_host(bad), "{bad}");
        }
    }

    #[test]
    fn host_ids_are_short_hex() {
        assert!(valid_id("ab12cd34"));
        assert!(valid_id("0"));
        // Hex is case-insensitive (new_id emits lowercase, but uppercase is still a valid id).
        assert!(valid_id("AB12"));
        for bad in ["", "xyz", "ab 12", "ab-12", &"a".repeat(33)] {
            assert!(!valid_id(bad), "{bad}");
        }
    }

    #[test]
    fn azure_location_and_subscription() {
        assert!(valid_azure_location("westeurope"));
        assert!(valid_azure_location("swedencentral"));
        for bad in ["", "West Europe", "west-europe", &"a".repeat(33)] {
            assert!(!valid_azure_location(bad), "{bad}");
        }
        assert!(valid_subscription("00000000-0000-0000-0000-000000000000"));
        assert!(valid_subscription("12345678-9abc-def0-1234-56789abcdef0"));
        for bad in ["", "12345678-9abc-def0-1234", "12345678-9abc-def0-1234-56789abcdefg", "123456789abcdef0123456789abcdef0"] {
            assert!(!valid_subscription(bad), "{bad}");
        }
    }

    #[test]
    fn gcp_region_billing_and_org() {
        assert!(valid_gcp_region("europe-west1"));
        assert!(valid_gcp_region("us-central1"));
        for bad in ["", "europewest1", "Europe-West1", &"a-".repeat(20)] {
            assert!(!valid_gcp_region(bad), "{bad}");
        }
        assert!(valid_billing_account("0X0X0X-0X0X0X-0X0X0X"));
        assert!(valid_billing_account("ABCDEF-123456-7890AB"));
        for bad in ["", "0x0x0x-0x0x0x-0x0x0x", "0X0X0X-0X0X0X", "0X0X0-0X0X0X-0X0X0X"] {
            assert!(!valid_billing_account(bad), "{bad}");
        }
        // Org id is digits, or empty for a personal account.
        assert!(valid_org_id(""));
        assert!(valid_org_id("123456789012"));
        assert!(!valid_org_id("12ab"));
        assert!(!valid_org_id(&"1".repeat(33)));
    }

    #[test]
    fn digitalocean_linode_regions_and_tokens() {
        assert!(valid_do_region("fra1"));
        assert!(valid_do_region("nyc3"));
        for bad in ["a", "ab", "NYC3", "fra-1", &"a".repeat(9)] {
            assert!(!valid_do_region(bad), "{bad}");
        }
        assert!(valid_linode_region("eu-central"));
        assert!(valid_linode_region("us-east"));
        for bad in ["", "ab", "9region", "EU-east", &"a".repeat(21)] {
            assert!(!valid_linode_region(bad), "{bad}");
        }
        // Tokens: 40..=200 chars of [A-Za-z0-9_].
        assert!(valid_do_token(&"a".repeat(40)));
        assert!(valid_do_token(&format!("dop_v1_{}", "9".repeat(64))));
        assert!(!valid_do_token(&"a".repeat(39)));
        assert!(!valid_do_token(&"a".repeat(201)));
        assert!(!valid_do_token(&format!("{}!", "a".repeat(40))));
    }

    #[test]
    fn oci_region_and_ocid() {
        assert!(valid_oci_region("eu-frankfurt-1"));
        assert!(valid_oci_region("us-ashburn-1"));
        for bad in ["eu", "EU-frankfurt-1", "1region", &"a".repeat(25)] {
            assert!(!valid_oci_region(bad), "{bad}");
        }
        assert!(valid_ocid("ocid1.tenancy.oc1..aaaaaaaabbbbbbbb"));
        assert!(valid_ocid("ocid1.compartment.oc1..aaaa_bbbb-cccc"));
        for bad in ["", "tenancy.oc1..aaaa", "ocid1.x", &format!("ocid1.{}", "a".repeat(260))] {
            assert!(!valid_ocid(bad), "{bad}");
        }
    }

    #[test]
    fn clean_trims_and_rejects_bad_values() {
        assert_eq!(clean("  lab  ", "name", 64).unwrap(), "lab");
        assert!(clean("", "name", 64).is_err());
        assert!(clean("   ", "name", 64).is_err());
        assert!(clean("toolong", "name", 3).is_err());
        assert!(clean("line\nbreak", "name", 64).is_err());
    }

    #[test]
    fn clean_opt_maps_blank_to_none() {
        assert_eq!(clean_opt(None, "x").unwrap(), None);
        assert_eq!(clean_opt(Some("  ".to_string()), "x").unwrap(), None);
        assert_eq!(clean_opt(Some(" vmbr1 ".to_string()), "x").unwrap(), Some("vmbr1".to_string()));
        assert!(clean_opt(Some("bad\tvalue".to_string()), "x").is_err());
    }

    #[test]
    fn check_refuses_local_providers_and_urls() {
        let local = input(serde_json::json!({"name": "x", "provider": "virtualbox", "host": "h", "username": "u"}));
        assert!(error(&local).contains("a host must be ESXi"));
        let url = input(serde_json::json!({"name": "x", "provider": "vmware_esxi", "host": "https://esxi", "username": "root"}));
        assert!(error(&url).contains("without https://"));
        let ok = input(serde_json::json!({"name": "x", "provider": "vmware_esxi", "host": " esxi.lan ", "username": "root"}));
        let c = ok.check().unwrap();
        assert_eq!((c.host.as_str(), c.username.as_str(), c.use_cli), ("esxi.lan", "root", false));
    }

    #[test]
    fn check_wants_a_realm_on_proxmox() {
        let no_realm = input(serde_json::json!({"name": "x", "provider": "proxmox", "host": "pve", "username": "root"}));
        assert!(error(&no_realm).contains("realm"));
        let token = input(serde_json::json!({"name": "x", "provider": "proxmox", "host": "pve", "username": "root@pam!cyberctf"}));
        assert!(token.check().is_ok());
    }

    #[test]
    fn check_steers_temporary_aws_keys_to_cli_mode() {
        let temp = input(serde_json::json!({"name": "x", "provider": "aws", "host": "eu-west-3", "username": "ASIAIOSFODNN7EXAMPLE"}));
        assert!(error(&temp).contains("temporary credentials"));
        let bad = input(serde_json::json!({"name": "x", "provider": "aws", "host": "eu-west-3", "username": "nope"}));
        assert!(error(&bad).contains("access key id looks wrong"));
        // CLI mode ignores the username entirely.
        let cli = input(serde_json::json!({"name": "x", "provider": "aws", "host": "eu-west-3", "username": "", "useCliCreds": true}));
        let c = cli.check().unwrap();
        assert!(c.use_cli && c.username.is_empty());
    }

    #[test]
    fn check_validates_tokens_only_when_given() {
        let keep = input(serde_json::json!({"name": "x", "provider": "digitalocean", "host": "fra1", "username": "", "password": ""}));
        assert!(keep.check().is_ok());
        let bad = input(serde_json::json!({"name": "x", "provider": "linode", "host": "eu-central", "username": "", "password": "short"}));
        assert!(error(&bad).contains("Linode API token"));
    }

    #[test]
    fn a_host_with_a_port_is_refused_and_only_an_ipv6_address_may_hold_colons() {
        // A player pasting `pve.lan:8006` (the address bar's host) gets past the form, and the
        // first connection fails on a bracketed URL with no hint about the port. Fix: when the
        // host holds a `:`, require it to parse as an `Ipv6Addr` (`host.parse::<Ipv6Addr>().is_ok()`),
        // and say "enter the port in its own field" otherwise.
        for bad in ["pve.lan:8006", "10.0.0.5:22", "esxi:"] {
            assert!(!valid_host(bad), "{bad}");
        }
        let i = input(serde_json::json!({"name": "x", "provider": "proxmox", "host": "pve.lan:8006", "username": "root@pam"}));
        assert!(i.check().is_err(), "a port in the host must be refused, not bracketed as an IPv6 address");
    }

    #[test]
    fn usernames_keep_spaces_quotes_and_dashes_but_not_control_characters_and_limits_are_bytes() {
        // An ESXi login only ever travels through the environment and a Vagrantfile string, so
        // a space, a quote or a leading dash is kept as typed; a tab or newline is refused.
        let esxi = |user: &str| input(serde_json::json!({"name": "x", "provider": "vmware_esxi", "host": "esxi", "username": user}));
        assert_eq!(esxi(" -root \"x\" ").check().unwrap().username, "-root \"x\"");
        assert_eq!(error(&esxi("root\tx")), "invalid username");
        assert_eq!(error(&esxi("root\u{7f}")), "invalid username");
        // 128 is a byte limit: 65 two-byte characters are 130 bytes.
        assert!(esxi(&"a".repeat(128)).check().is_ok());
        assert_eq!(error(&esxi(&"a".repeat(129))), "invalid username");
        assert_eq!(error(&esxi(&"\u{e9}".repeat(65))), "invalid username");
        // A Proxmox realm is any `@`, even alone or with a token part.
        let pve = |user: &str| input(serde_json::json!({"name": "x", "provider": "proxmox", "host": "pve", "username": user}));
        assert!(pve("@").check().is_ok() && pve("a@b!c").check().is_ok());
    }

    #[test]
    fn optional_names_are_trimmed_kept_verbatim_and_bounded() {
        // Datastore and network names go to the Vagrantfile/Terraform as variables, never argv:
        // spaces and parentheses stay; an aws profile is only kept in CLI mode.
        let i = input(serde_json::json!({
            "name": "x", "provider": "vmware_esxi", "host": "esxi", "username": "root",
            "datastore": "  my datastore (ssd)  ", "network": "VM Network", "node": "ignored", "awsProfile": "work", "useCliCreds": true
        }));
        let c = i.check().unwrap();
        assert!(!c.use_cli, "CLI credentials are AWS only");
        let p = i.into_profile("ab".into(), c, None).unwrap();
        assert_eq!((p.datastore.as_deref(), p.network.as_deref(), p.node, p.aws_profile), (Some("my datastore (ssd)"), Some("VM Network"), None, None));
        let i = input(serde_json::json!({"name": "x", "provider": "vmware_esxi", "host": "esxi", "username": "root", "datastore": "a".repeat(129)}));
        let c = i.check().unwrap();
        assert_eq!(i.into_profile("ab".into(), c, None).unwrap_err().to_string(), "invalid datastore");
        // The display name is bounded at 64 and may hold anything printable.
        let i = input(serde_json::json!({"name": "\u{1f980} Lab \"one\"", "provider": "vmware_esxi", "host": "esxi", "username": "root"}));
        let c = i.check().unwrap();
        assert_eq!(i.into_profile("ab".into(), c, None).unwrap().name, "\u{1f980} Lab \"one\"");
    }

    #[test]
    fn auto_stop_and_budget_at_their_bounds() {
        let aws = |extra: serde_json::Value| {
            let mut v = serde_json::json!({"name": "x", "provider": "aws", "host": "eu-west-3", "username": "AKIAIOSFODNN7EXAMPLE"});
            v.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
            let i = input(v);
            let c = i.check().unwrap();
            i.into_profile("ab".into(), c, None)
        };
        assert_eq!(aws(serde_json::json!({"autoStopHours": 0})).unwrap().auto_stop_hours, Some(0), "0 means never");
        assert_eq!(aws(serde_json::json!({"autoStopHours": 72})).unwrap().auto_stop_hours, Some(72));
        assert_eq!(aws(serde_json::json!({"autoStopHours": 73})).unwrap_err().to_string(), "auto-stop must be between 0 and 72 hours");
        assert_eq!(aws(serde_json::json!({"monthlyLimit": -1.0})).unwrap().monthly_limit, None);
        assert_eq!(aws(serde_json::json!({"monthlyLimit": 0.01})).unwrap().monthly_limit, Some(0.01));
        // A port of 0 is "unset": the provider's default.
        assert_eq!(aws(serde_json::json!({"port": 0})).unwrap().port, 443);
    }

    #[test]
    fn into_profile_applies_provider_defaults() {
        let i = input(
            serde_json::json!({"name": " Box ", "provider": "aws", "host": "eu-west-3", "username": "AKIAIOSFODNN7EXAMPLE", "node": "ignored", "monthlyLimit": 0.0, "insecureTls": true}),
        );
        let c = i.check().unwrap();
        let p = i.into_profile("ab".into(), c, None).unwrap();
        assert_eq!(p.name, "Box");
        assert_eq!(p.port, 443);
        assert_eq!(p.auto_stop_hours, Some(DEFAULT_AUTO_STOP_HOURS));
        // Fields that don't apply to the provider are dropped.
        assert_eq!(p.node, None);
        assert!(!p.insecure_tls);
        assert_eq!(p.monthly_limit, None);

        let i = input(serde_json::json!({"name": "pve", "provider": "proxmox", "host": "pve", "username": "root@pam", "node": "n1", "autoStopHours": 99}));
        let c = i.check().unwrap();
        let p = i.into_profile("ab".into(), c, None).unwrap();
        assert_eq!((p.port, p.node.as_deref(), p.auto_stop_hours), (8006, Some("n1"), None));

        let i = input(
            serde_json::json!({"name": "a", "provider": "azure", "host": "westeurope", "username": "00000000-0000-0000-0000-000000000000", "autoStopHours": 73}),
        );
        let c = i.check().unwrap();
        assert!(i.into_profile("ab".into(), c, None).is_err());
    }
}

#[cfg(test)]
mod proptests;
