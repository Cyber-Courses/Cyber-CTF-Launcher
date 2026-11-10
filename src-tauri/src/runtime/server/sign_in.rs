//! The one question that tells whether a cloud account's sign-in still holds, asked of its CLI. A
//! launch asks it before it changes anything, an account's Test asks it first, and the cloud
//! setup asks it before it says "signed in as …": the three agree. (Each asked its own, and Test
//! and the setup could pass on Google Cloud's user sign-in while every launch was refused because
//! the application-default sign-in Terraform uses had run out.)

/// AWS's: who its credentials are, as the identity's ARN.
pub(crate) const AWS_WHO_AM_I: [&str; 6] = ["sts", "get-caller-identity", "--query", "Arn", "--output", "text"];

/// The question for `cloud` (the launcher's Terraform target: `aws`, `azure`, `gcp`, `oci`): who
/// AWS's credentials are, an Azure CLI token, Google's application-default token (what its
/// Terraform provider signs in with), Oracle's regions (through `~/.oci/config`). `DigitalOcean`
/// and Linode sign in with a stored API token, which doesn't run out: none.
pub(crate) fn sign_in_question(cloud: &str) -> Option<(&'static str, &'static [&'static str])> {
    Some(match cloud {
        "aws" => ("aws", &AWS_WHO_AM_I),
        "azure" => ("az", &["account", "get-access-token", "--output", "none"]),
        "gcp" => ("gcloud", &["auth", "application-default", "print-access-token"]),
        "oci" => ("oci", &["iam", "region", "list", "--output", "json"]),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_cloud_that_signs_in_through_its_cli_has_one_question() {
        assert_eq!(sign_in_question("aws"), Some(("aws", &AWS_WHO_AM_I[..])));
        assert_eq!(sign_in_question("gcp").map(|(p, a)| (p, a.join(" "))), Some(("gcloud", "auth application-default print-access-token".into())));
        assert!(sign_in_question("azure").is_some() && sign_in_question("oci").is_some());
        assert_eq!(sign_in_question("digitalocean"), None);
        assert_eq!(sign_in_question("linode"), None);
    }
}
