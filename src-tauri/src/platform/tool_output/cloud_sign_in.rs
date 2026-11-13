//! What a cloud's tools say when the sign-in they use has run out or is missing, read: Terraform's
//! providers and the AWS, Azure, Google Cloud and Oracle Cloud CLIs, from a CLI login or SSO
//! session, an expired session token, application-default credentials gone stale, or a profile
//! that was never signed in.

/// What a cloud's tools say when the sign-in they use has run out or is missing: Terraform's
/// providers and the clouds' CLIs, from a CLI login or SSO session, an expired session token,
/// application-default credentials gone stale, or a profile that was never signed in.
pub const EXPIRED_SIGN_IN: [&str; 21] = [
    // AWS
    "login session has expired",
    "please reauthenticate",
    "expiredtoken",
    "security token included in the request is expired",
    "token has expired",
    "error loading sso token",
    "unable to locate credentials",
    "aws login",
    // Azure
    "az login",
    "aadsts700082",
    "aadsts50173",
    "refresh token has expired",
    "interactive authentication is needed",
    // Google Cloud
    "reauthentication failed",
    "problem refreshing your current auth tokens",
    "gcloud auth application-default login",
    "could not find default credentials",
    "invalid_grant",
    // Oracle Cloud
    "session is no longer valid",
    "oci session authenticate",
    "notauthenticated",
];

/// Whether `reason`, what a cloud tool printed, says its sign-in ran out or is missing.
#[must_use]
pub fn expired(reason: &str) -> bool {
    let lower = reason.to_ascii_lowercase();
    EXPIRED_SIGN_IN.iter().any(|s| lower.contains(s))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_clouds_expired_sign_in_is_read() {
        for said in [
            "Error: operation error STS: GetCallerIdentity, https response error StatusCode: 400, api error ExpiredToken: The security token included in the request is expired",
            "Error loading SSO Token: Token for my-sso does not exist",
            "ERROR: AADSTS700082: The refresh token has expired due to inactivity.",
            "ERROR: (gcloud.compute.instances.list) There was a problem refreshing your current auth tokens: Reauthentication failed. cannot prompt during non-interactive execution.",
            "ServiceError: {'code': 'NotAuthenticated', 'message': 'The required information to complete authentication was not provided.'}",
        ] {
            assert!(expired(said), "{said}");
        }
        assert!(!expired("Error: creating EC2 Instance: InsufficientInstanceCapacity"));
        assert!(!expired(""));
    }
}
