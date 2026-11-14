//! What the setup form submits, and turning it into a `HostProfile`: every field is checked
//! against its provider's shape (region ids, account ids, tokens) before anything is saved.

use serde::Deserialize;
use ts_rs::TS;

use super::kind::{AccountField, Secret, kind};
use super::profile::{DEFAULT_AUTO_STOP_HOURS, HostProfile, SshKeySignIn, default_port};
use crate::error::{Error, Result};
use crate::runtime::providers::Provider;

/// What the UI submits. `id` is None for a new host; `password` None keeps the stored one.
#[derive(Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "ServerHostInput")]
pub struct HostInput {
    /// The host's id; None for a new host.
    pub(crate) id: Option<String>,
    name: String,
    /// Which kind of host it is.
    pub(crate) provider: Provider,
    host: String,
    port: Option<u16>,
    username: String,
    datastore: Option<String>,
    network: Option<String>,
    node: Option<String>,
    insecure_tls: Option<bool>,
    /// Proxmox with a self-signed certificate: the fingerprint the learner confirmed for it.
    #[serde(default)]
    #[ts(optional = nullable)]
    tls_fingerprint: Option<String>,
    auto_stop_hours: Option<u32>,
    /// Its secret (a password, a token's secret, a secret key or an API token); None keeps the
    /// stored one.
    pub(crate) password: Option<String>,
    #[serde(default)]
    #[ts(optional = nullable)]
    use_cli_creds: Option<bool>,
    #[serde(default)]
    #[ts(optional = nullable)]
    aws_profile: Option<String>,
    #[serde(default)]
    #[ts(optional = nullable)]
    monthly_limit: Option<f64>,
    /// Proxmox: sign in to the node over SSH with this key to make the launcher's API token
    /// (`server_save_ssh_key`); `username` and `password` are then the token's.
    #[serde(default)]
    #[ts(optional = nullable)]
    ssh_key: Option<SshKeySignIn>,
    /// Proxmox: where cloud images and cloud-init snippets go (see `HostProfile`).
    #[serde(default)]
    #[ts(optional = nullable)]
    image_storage: Option<String>,
    #[serde(default)]
    #[ts(optional = nullable)]
    snippet_storage: Option<String>,
}

/// The connection fields of an input, checked: what `HostInput::into_profile` builds on.
pub(crate) struct Checked {
    /// The host's address, or a cloud's region.
    pub host: String,
    /// The account field (`AccountField`): a login, a key id or where labs are created.
    pub username: String,
    /// AWS through the AWS CLI's own credentials (no stored keys).
    pub use_cli: bool,
}

fn require(ok: bool, message: &str) -> Result<()> {
    if ok { Ok(()) } else { Err(Error::Invalid(message.into())) }
}

impl HostInput {
    /// The SSH key sign-in this Proxmox input asks for, checked: an existing private key file
    /// (not its `.pub`), a plain user name and a port.
    pub(crate) fn ssh_key(&self) -> Result<SshKeySignIn> {
        let key =
            self.ssh_key.as_ref().filter(|_| self.provider == Provider::Proxmox).ok_or_else(|| Error::Invalid("choose the SSH key to sign in with".into()))?;
        checked_ssh_key(key)
    }

    /// Whether this input signs in as the saved `host` does with the same SSH key: a save of an
    /// SSH-key setup whose token was already made keeps its key.
    pub(crate) fn same_ssh_key(&self, host: &HostProfile) -> bool {
        self.ssh_key.is_some() && self.ssh_key == host.ssh_key && crate::runtime::proxmox::with_default_realm(self.username.trim()) == host.username
    }

    /// The server's address, checked (a hostname or IP, no scheme or path).
    pub(crate) fn address(&self) -> Result<String> {
        let host = clean(&self.host, "host", 253)?;
        require(valid_host(&host), "host must be a hostname or IP address, without https:// or a path")?;
        Ok(host)
    }

    /// The port the form names, if any.
    pub(crate) fn port(&self) -> Option<u16> {
        self.port.filter(|p| *p > 0)
    }

    /// Whether this is a Proxmox server with a self-signed certificate.
    pub(crate) fn self_signed(&self) -> bool {
        self.provider == Provider::Proxmox && self.insecure_tls.unwrap_or(false)
    }

    /// Drops any SSH key sign-in: a password or token save keeps none.
    pub(crate) fn without_ssh_key(&mut self) {
        self.ssh_key = None;
    }

    /// Signs in as `username` with `password` (an SSH-key setup's token, once made).
    pub(crate) fn sign_in_as(&mut self, username: String, password: Option<String>) {
        self.username = username;
        self.password = password;
    }

    /// The certificate fingerprint the learner confirmed, checked; None when the host isn't a
    /// Proxmox server with a self-signed certificate.
    pub(crate) fn tls_fingerprint(&self) -> Result<Option<crate::runtime::server::tls_pin::Fingerprint>> {
        if self.provider != Provider::Proxmox || !self.insecure_tls.unwrap_or(false) {
            return Ok(None);
        }
        match self.tls_fingerprint.as_deref().map(str::trim).filter(|f| !f.is_empty()) {
            None => Ok(None),
            Some(f) => crate::runtime::server::tls_pin::Fingerprint::parse(f)
                .map(Some)
                .ok_or_else(|| Error::Invalid("the certificate fingerprint must be its 64 hex digits (SHA-256)".into())),
        }
    }

    /// Checks the connection fields against the provider (address or region, account,
    /// credentials shape), from its row in the table of kinds (see `kind`).
    pub(crate) fn check(&self) -> Result<Checked> {
        let kind = kind(self.provider).ok_or_else(|| Error::Invalid("a host must be ESXi, Proxmox, AWS, Azure, GCP, DigitalOcean, Linode or OCI".into()))?;
        let host = clean(&self.host, "host", 253)?;
        // AWS can connect with the AWS CLI's own credentials instead of stored keys.
        let use_cli = kind.account == AccountField::AccessKeyId && self.use_cli_creds.unwrap_or(false);
        // A token is the whole credential (DigitalOcean, Linode): no username.
        let username = if use_cli || kind.account == AccountField::Unused { String::new() } else { clean(&self.username, "username", 128)? };
        // Proxmox: `root` is `root@pam` (and `root!name` its token), the realm most setups use.
        let username = if self.provider == Provider::Proxmox { crate::runtime::proxmox::with_default_realm(&username) } else { username };
        match &kind.region {
            Some(region) => require((region.valid)(&host), region.message)?,
            None => require(valid_host(&host), "host must be a hostname or IP address, without https:// or a path")?,
        }
        if kind.account == AccountField::AccessKeyId && !use_cli {
            require(
                !username.starts_with("ASIA"),
                "those are temporary credentials (ASIA...), which expire and need a session token. Switch to \"Connect with the AWS CLI\" and sign in with `aws login` / `aws sso login` so the launcher always has fresh credentials.",
            )?;
        }
        if let Some(account) = kind.account_shape.as_ref().filter(|_| !username.is_empty()) {
            require((account.valid)(&username), account.message)?;
        }
        // A new token must look like one; None or empty keeps the stored one.
        if kind.secret == Secret::ApiToken {
            let token_ok = self.password.as_deref().filter(|p| !p.is_empty()).is_none_or(valid_do_token);
            require(token_ok, &format!("that doesn't look like a {} API token", kind.provider.label()))?;
        }
        if self.provider == Provider::Gcp {
            require(valid_org_id(self.node.as_deref().unwrap_or_default()), "organization id must be all digits, or empty for a personal account")?;
        }
        Ok(Checked { host, username, use_cli })
    }

    /// The profile to save under `id`. `gcp_project` is the labs project kept from the previous
    /// save, if any.
    pub(crate) fn into_profile(self, id: String, checked: Checked, gcp_project: Option<String>) -> Result<HostProfile> {
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
            datastore: clean_opt(self.datastore.as_deref(), "datastore")?,
            network: clean_opt(self.network.as_deref(), "network")?,
            // Proxmox uses `node`; GCP reuses it for the optional organization id.
            node: if matches!(provider, Provider::Proxmox | Provider::Gcp) { clean_opt(self.node.as_deref(), "node")? } else { None },
            insecure_tls: provider == Provider::Proxmox && self.insecure_tls.unwrap_or(false),
            tls_fingerprint: self.tls_fingerprint()?,
            auto_stop_hours: if provider.is_cloud() {
                match self.auto_stop_hours.unwrap_or(DEFAULT_AUTO_STOP_HOURS) {
                    h if h <= 72 => Some(h),
                    _ => return Err(Error::Invalid("auto-stop must be between 0 and 72 hours".into())),
                }
            } else {
                None
            },
            use_cli_creds: use_cli,
            aws_profile: if use_cli { clean_opt(self.aws_profile.as_deref(), "profile")? } else { None },
            // Budget enforcement is AWS-only (the other clouds have no cost read yet), so don't
            // persist a limit that would silently do nothing on Azure/GCP.
            monthly_limit: if provider == Provider::Aws { self.monthly_limit.filter(|v| *v > 0.0) } else { None },
            ssh_key: if provider == Provider::Proxmox { self.ssh_key.as_ref().map(checked_ssh_key).transpose()? } else { None },
            image_storage: if provider == Provider::Proxmox { clean_opt(self.image_storage.as_deref(), "image storage")? } else { None },
            snippet_storage: if provider == Provider::Proxmox { clean_opt(self.snippet_storage.as_deref(), "snippet storage")? } else { None },
        })
    }
}

// --- field shapes ---------------------------------------------------------

/// An SSH key sign-in as the form gave it, checked (see `HostInput::ssh_key`).
fn checked_ssh_key(key: &SshKeySignIn) -> Result<SshKeySignIn> {
    let path = key.path.trim();
    require(!path.is_empty() && path.len() <= 1024 && !path.chars().any(char::is_control), "choose the SSH key to sign in with")?;
    require(std::path::Path::new(path).is_absolute(), "the SSH key must be a full path")?;
    let public = std::path::Path::new(path).extension().is_some_and(|e| e.eq_ignore_ascii_case("pub"));
    require(!public, "choose the private key, not its .pub file")?;
    require(std::path::Path::new(path).is_file(), &format!("there is no SSH key at {path}"))?;
    let user = key.user.trim();
    require(valid_ssh_user(user), "the SSH user must be a plain user name, e.g. root")?;
    require(key.port > 0, "the SSH port must be between 1 and 65535")?;
    Ok(SshKeySignIn { path: path.to_string(), user: user.to_string(), port: key.port })
}

/// A Unix user name we put on an ssh command line and in a `pveum` call: letters, digits,
/// `.`, `_` and `-`, not starting with `-`.
fn valid_ssh_user(user: &str) -> bool {
    !user.is_empty() && user.len() <= 32 && !user.starts_with('-') && user.chars().all(|c| c.is_ascii_alphanumeric() || "._-".contains(c))
}

pub(super) fn clean(value: &str, field: &str, max: usize) -> Result<String> {
    let v = value.trim();
    if v.is_empty() || v.len() > max || v.chars().any(char::is_control) {
        return Err(Error::Invalid(format!("invalid {field}")));
    }
    Ok(v.to_string())
}

pub(super) fn clean_opt(value: Option<&str>, field: &str) -> Result<Option<String>> {
    match value.map(str::trim) {
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
pub(crate) fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 32 && id.chars().all(|c| c.is_ascii_hexdigit())
}

/// An AWS region id, e.g. eu-west-3, us-gov-west-1.
pub(super) fn valid_region(region: &str) -> bool {
    let parts: Vec<&str> = region.split('-').collect();
    parts.len() >= 3
        && parts[0].len() == 2
        && parts.iter().all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()))
        && parts.last().is_some_and(|p| p.chars().all(|c| c.is_ascii_digit()))
}

/// An Azure location id, e.g. "westeurope" (lowercase letters/digits, no spaces).
pub(super) fn valid_azure_location(s: &str) -> bool {
    !s.is_empty() && s.len() <= 32 && s.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
}

/// An Azure subscription id (a GUID, 8-4-4-4-12 hex).
pub(super) fn valid_subscription(s: &str) -> bool {
    let p: Vec<&str> = s.split('-').collect();
    p.len() == 5 && [8usize, 4, 4, 4, 12].iter().zip(&p).all(|(n, seg)| seg.len() == *n && seg.chars().all(|c| c.is_ascii_hexdigit()))
}

/// A GCP region id, e.g. "europe-west1", "us-central1" (lowercase letters/digits with a hyphen,
/// starting with a letter so it never reads as a flag).
pub(super) fn valid_gcp_region(s: &str) -> bool {
    s.len() <= 32 && s.contains('-') && starts_lower_slug(s)
}

/// A GCP billing account id: three 6-char groups of uppercase hex, e.g. 0X0X0X-0X0X0X-0X0X0X.
pub(super) fn valid_billing_account(s: &str) -> bool {
    let p: Vec<&str> = s.split('-').collect();
    p.len() == 3 && p.iter().all(|g| g.len() == 6 && g.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit()))
}

/// A GCP organization id: all digits (e.g. 123456789012). Empty = personal / no-org account.
fn valid_org_id(s: &str) -> bool {
    s.is_empty() || (s.len() <= 32 && s.chars().all(|c| c.is_ascii_digit()))
}

/// A `DigitalOcean` region slug, e.g. "fra1", "nyc3" (lowercase letters then digits).
pub(super) fn valid_do_region(s: &str) -> bool {
    (3..=8).contains(&s.len()) && s.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
}

/// A `DigitalOcean` / Linode API token: the "`dop_v1`_..." form or a hex PAT. Loose length/charset check.
fn valid_do_token(s: &str) -> bool {
    (40..=200).contains(&s.len()) && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// A Linode region slug, e.g. "eu-central", "us-east", "ap-south" (lowercase letters/digits/hyphens).
pub(super) fn valid_linode_region(s: &str) -> bool {
    (3..=20).contains(&s.len()) && starts_lower_slug(s)
}

/// An OCI region id, e.g. "eu-frankfurt-1", "us-ashburn-1" (lowercase letters/digits/hyphens).
pub(super) fn valid_oci_region(s: &str) -> bool {
    (5..=24).contains(&s.len()) && starts_lower_slug(s)
}

/// Lowercase letters, digits and hyphens, starting with a letter.
fn starts_lower_slug(s: &str) -> bool {
    s.starts_with(|c: char| c.is_ascii_lowercase()) && s.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// An OCID (tenancy / compartment / ...): starts with "ocid1." then dot/hyphen-separated alnum.
pub(super) fn valid_ocid(s: &str) -> bool {
    s.starts_with("ocid1.") && (20..=255).contains(&s.len()) && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_')
}

/// A long-term AWS access key id (AKIA...). Temporary ASIA... keys are refused separately in
/// `check`: they need a session token and expire, so the launcher steers them to CLI mode.
pub(super) fn valid_access_key_id(id: &str) -> bool {
    (16..=128).contains(&id.len()) && id.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod proptests;
