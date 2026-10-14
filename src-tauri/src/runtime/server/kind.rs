//! Each kind of host in one table: what a profile's shared fields mean for it (`host` is an
//! address or a region, `username` a login, a key id or the account labs are created in,
//! `datastore` a size on a cloud), how it is checked, which Terraform target and size variable
//! its labs use, what secret the keychain keeps for it, and the environment Terraform gets.
//! Adding a provider is adding a row here, not editing a `match` in every file.

use super::contract::{aws_env, azure_env, digitalocean_env, linode_env, no_env};
use super::input::{
    valid_access_key_id, valid_azure_location, valid_billing_account, valid_do_region, valid_gcp_region, valid_linode_region, valid_oci_region, valid_ocid,
    valid_region, valid_subscription,
};
use super::profile::HostProfile;
use crate::runtime::providers::Provider;

/// What a profile's `host` field holds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HostField {
    /// The server's hostname or IP address.
    Address,
    /// The cloud region labs are created in.
    Region,
}

/// What a profile's `username` field holds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AccountField {
    /// A login: an ESXi user, a Proxmox `user@realm` (or token id).
    Login,
    /// An AWS access key id (empty with the AWS CLI's own credentials).
    AccessKeyId,
    /// The Azure subscription labs are created in.
    Subscription,
    /// The GCP billing account the labs project is linked to.
    BillingAccount,
    /// The OCI compartment labs are created in.
    Compartment,
    /// Nothing: the API token is the whole credential.
    Unused,
}

impl AccountField {
    /// Whether it says where labs are (the account they are created in), rather than how to
    /// sign in: changing it moves the account, changing a login or key doesn't.
    pub fn is_placement(self) -> bool {
        matches!(self, AccountField::Subscription | AccountField::BillingAccount | AccountField::Compartment)
    }
}

/// The secret the keychain keeps for a host.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Secret {
    /// The login's password (a Proxmox token's secret).
    Password,
    /// An AWS secret access key (none with the AWS CLI's own credentials).
    AccessKey,
    /// An API token.
    ApiToken,
    /// Nothing: the cloud's own CLI or config file signs in.
    None,
}

/// A field's shape, and what the setup form says when it doesn't have it.
pub struct Shape {
    pub valid: fn(&str) -> bool,
    pub message: &'static str,
}

/// One kind of host.
pub struct Kind {
    pub provider: Provider,
    /// Its name for the player.
    pub name: &'static str,
    /// The Terraform target (state folder, Isoloom module) its labs use; None for Vagrant.
    pub terraform: Option<&'static str>,
    pub default_port: u16,
    pub host: HostField,
    /// The region's shape (clouds); a server's address is checked as one.
    pub region: Option<Shape>,
    pub account: AccountField,
    pub account_shape: Option<Shape>,
    /// The Isoloom module variable the instance size (`datastore`) goes to (clouds).
    pub size_var: Option<&'static str>,
    pub secret: Secret,
    /// The raw environment Terraform gets (credentials the provider reads itself), from the
    /// profile and its secret.
    pub terraform_env: fn(&HostProfile, &str) -> Vec<(String, String)>,
}

const fn shape(valid: fn(&str) -> bool, message: &'static str) -> Option<Shape> {
    Some(Shape { valid, message })
}

/// Every kind of host the launcher runs labs on.
pub const KINDS: [Kind; 8] = [
    Kind {
        provider: Provider::VmwareEsxi,
        name: "ESXi",
        terraform: None,
        default_port: 22,
        host: HostField::Address,
        region: None,
        account: AccountField::Login,
        account_shape: None,
        size_var: None,
        secret: Secret::Password,
        terraform_env: no_env,
    },
    Kind {
        provider: Provider::Proxmox,
        name: "Proxmox",
        terraform: Some("proxmox"),
        default_port: 8006,
        host: HostField::Address,
        region: None,
        account: AccountField::Login,
        account_shape: shape(|u| u.contains('@'), "Proxmox users include a realm, e.g. root@pam"),
        size_var: None,
        secret: Secret::Password,
        terraform_env: no_env,
    },
    Kind {
        provider: Provider::Aws,
        name: "AWS",
        terraform: Some("aws"),
        default_port: 443,
        host: HostField::Region,
        region: shape(valid_region, "region must be an AWS region id, e.g. eu-west-3"),
        account: AccountField::AccessKeyId,
        account_shape: shape(valid_access_key_id, "access key id looks wrong; an IAM user's key starts with AKIA..."),
        size_var: Some("instance_type"),
        secret: Secret::AccessKey,
        terraform_env: aws_env,
    },
    Kind {
        provider: Provider::Azure,
        name: "Azure",
        terraform: Some("azure"),
        default_port: 443,
        host: HostField::Region,
        region: shape(valid_azure_location, "region must be an Azure location, e.g. westeurope"),
        account: AccountField::Subscription,
        account_shape: shape(valid_subscription, "subscription must be a GUID (see `az account show`)"),
        size_var: Some("size"),
        secret: Secret::None,
        terraform_env: azure_env,
    },
    Kind {
        provider: Provider::Gcp,
        name: "GCP",
        terraform: Some("gcp"),
        default_port: 443,
        host: HostField::Region,
        region: shape(valid_gcp_region, "region must be a GCP region id, e.g. europe-west1"),
        account: AccountField::BillingAccount,
        account_shape: shape(valid_billing_account, "billing account must look like 0X0X0X-0X0X0X-0X0X0X (see `gcloud billing accounts list`)"),
        size_var: Some("machine_type"),
        secret: Secret::None,
        // The google provider uses gcloud's ADC; the project comes as a variable (see `gcp`).
        terraform_env: no_env,
    },
    Kind {
        provider: Provider::DigitalOcean,
        name: "DigitalOcean",
        terraform: Some("digitalocean"),
        default_port: 443,
        host: HostField::Region,
        region: shape(valid_do_region, "region must be a DigitalOcean region slug, e.g. fra1 or nyc3"),
        account: AccountField::Unused,
        account_shape: None,
        size_var: Some("size"),
        secret: Secret::ApiToken,
        terraform_env: digitalocean_env,
    },
    Kind {
        provider: Provider::Linode,
        name: "Linode",
        terraform: Some("linode"),
        default_port: 443,
        host: HostField::Region,
        region: shape(valid_linode_region, "region must be a Linode region slug, e.g. eu-central or us-east"),
        account: AccountField::Unused,
        account_shape: None,
        size_var: Some("type"),
        secret: Secret::ApiToken,
        terraform_env: linode_env,
    },
    Kind {
        provider: Provider::Oci,
        name: "OCI",
        terraform: Some("oci"),
        default_port: 443,
        host: HostField::Region,
        region: shape(valid_oci_region, "region must be an OCI region id, e.g. eu-frankfurt-1"),
        account: AccountField::Compartment,
        account_shape: shape(valid_ocid, "compartment must be an OCID (ocid1.compartment... or the tenancy OCID)"),
        size_var: Some("instance_type"),
        secret: Secret::None,
        terraform_env: no_env,
    },
];

/// The kind of host `provider` is; None for a hypervisor on this machine.
pub fn kind(provider: Provider) -> Option<&'static Kind> {
    KINDS.iter().find(|k| k.provider == provider)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_host_provider_has_one_row_and_no_local_one_has_any() {
        for p in
            [Provider::VmwareEsxi, Provider::Proxmox, Provider::Aws, Provider::Azure, Provider::Gcp, Provider::DigitalOcean, Provider::Linode, Provider::Oci]
        {
            assert_eq!(KINDS.iter().filter(|k| k.provider == p).count(), 1, "{p:?}");
            let k = kind(p).unwrap();
            // A cloud is reached at a region, through Terraform, and says how big its instance is.
            assert_eq!(p.is_cloud(), k.host == HostField::Region && k.region.is_some() && k.size_var.is_some(), "{p:?}");
            assert!(p.is_cloud() == (k.default_port == 443), "{p:?}");
        }
        for p in Provider::ALL.iter().filter(|p| !p.is_remote()) {
            assert!(kind(*p).is_none(), "{p:?}");
        }
    }

    #[test]
    fn only_the_account_labs_are_created_in_places_them() {
        let placement: Vec<Provider> = KINDS.iter().filter(|k| k.account.is_placement()).map(|k| k.provider).collect();
        assert_eq!(placement, [Provider::Azure, Provider::Gcp, Provider::Oci]);
        // A login or an access key is how to sign in: renewing it doesn't move the account.
        assert!(!AccountField::Login.is_placement() && !AccountField::AccessKeyId.is_placement() && !AccountField::Unused.is_placement());
    }

    #[test]
    fn the_shapes_accept_their_examples() {
        let examples = [
            (Provider::Aws, "eu-west-3", "AKIAIOSFODNN7EXAMPLE"),
            (Provider::Azure, "westeurope", "00000000-0000-0000-0000-000000000000"),
            (Provider::Gcp, "europe-west1", "0X0X0X-0X0X0X-0X0X0X"),
            (Provider::Oci, "eu-frankfurt-1", "ocid1.tenancy.oc1..aaaaaaaabbbbbbbb"),
            (Provider::Proxmox, "", "root@pam"),
        ];
        for (p, region, account) in examples {
            let k = kind(p).unwrap();
            assert!(k.region.as_ref().is_none_or(|s| (s.valid)(region)), "{p:?} {region}");
            assert!(k.account_shape.as_ref().is_some_and(|s| (s.valid)(account)), "{p:?} {account}");
        }
    }
}
