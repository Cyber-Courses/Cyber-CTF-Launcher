//! What `gcloud` prints when a call fails, read: the status Google's API answered with, which it
//! names in capitals in every message (`ERROR: (gcloud.projects.describe) NOT_FOUND: …`), and a
//! billing link refused for want of a project slot.

/// Whether `stderr`, a failed `gcloud` call's, says the API answered `NOT_FOUND`.
#[must_use]
pub fn not_found(stderr: &str) -> bool {
    stderr.split(|c: char| !(c.is_ascii_uppercase() || c == '_')).any(|word| word == "NOT_FOUND")
}

/// Whether `why`, a refused `gcloud billing projects link`'s, says the billing account has no
/// free project slot (a quota, or a failed precondition, as Google words it).
#[must_use]
pub fn no_project_slot(why: &str) -> bool {
    why.contains("quota") || why.contains("FAILED_PRECONDITION") || why.contains("Precondition")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failures_read_as_gcloud_prints_them() {
        assert!(not_found("ERROR: (gcloud.projects.describe) NOT_FOUND: Project cyberctf-labs-1234 not found or permission denied.\r\n"));
        assert!(!not_found("ERROR: (gcloud.projects.describe) PERMISSION_DENIED: The caller does not have permission"));
        assert!(!not_found("ERROR: something NOT_FOUNDATION"), "a word, not a part of one");
        assert!(no_project_slot(
            "ERROR: (gcloud.billing.projects.link) FAILED_PRECONDITION: Precondition check failed.\n- '@type': type.googleapis.com/google.rpc.QuotaFailure"
        ));
        assert!(no_project_slot("Cloud billing quota exceeded"));
        assert!(!no_project_slot("ERROR: (gcloud.billing.projects.link) PERMISSION_DENIED"));
    }
}
