//! The one question that tells whether a cloud account's sign-in still holds, asked of its CLI. A
//! launch asks it before it changes anything, an account's Test asks it first, and the cloud
//! setup asks it before it says "signed in as …": the three agree. (Each asked its own, and Test
//! and the setup could pass on Google Cloud's user sign-in while every launch was refused because
//! the application-default sign-in Terraform uses had run out.)

use crate::runtime::providers::Provider;

/// AWS's: who its credentials are, as the identity's ARN.
pub(crate) const AWS_WHO_AM_I: [&str; 6] = ["sts", "get-caller-identity", "--query", "Arn", "--output", "text"];

/// The question for `cloud`, its row's ([`super::kind::CliSignIn`]): who AWS's credentials are,
/// an Azure CLI token, Google's application-default token (what its Terraform provider signs in
/// with), Oracle's regions (through `~/.oci/config`). `DigitalOcean` and Linode sign in with a
/// stored API token, which doesn't run out: none.
pub(crate) fn sign_in_question(cloud: Provider) -> Option<(&'static str, &'static [&'static str])> {
    super::kind::kind(cloud)?.cli_sign_in.as_ref().map(|s| s.question)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_cloud_that_signs_in_through_its_cli_has_one_question() {
        assert_eq!(sign_in_question(Provider::Aws), Some(("aws", &AWS_WHO_AM_I[..])));
        assert_eq!(sign_in_question(Provider::Gcp).map(|(p, a)| (p, a.join(" "))), Some(("gcloud", "auth application-default print-access-token".into())));
        assert!(sign_in_question(Provider::Azure).is_some() && sign_in_question(Provider::Oci).is_some());
        assert_eq!(sign_in_question(Provider::DigitalOcean), None);
        assert_eq!(sign_in_question(Provider::Linode), None);
        assert_eq!(sign_in_question(Provider::Proxmox), None);
    }
}
