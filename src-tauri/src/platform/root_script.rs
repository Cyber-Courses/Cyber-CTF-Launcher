//! Pieces of the shell scripts run as root that download something and trust it only once it is
//! checked: a fresh private work directory, gpg with a keyring of its own there, and the check of
//! a downloaded signing key against the fingerprints pinned for it. The vendors' apt repositories
//! a Linux machine sets up (`machine::tools::scripts`) and Kali's archive a Debian attack VM is
//! made Kali from (`runtime::attack_machine`) go through the same check.

/// The start of every script run as root that downloads something: stop at the first failure,
/// and work in a fresh private directory `$d` (`mktemp -d`, mode 0700, removed on exit). A fixed
/// path such as `/tmp/aws` could be prepared by any local user beforehand and then run as root.
pub const PRIVATE_WORKDIR: &str = r#"set -e; d="$(mktemp -d)"; trap 'rm -rf "$d"' EXIT"#;

/// curl, installed first where a bare system has none (Debian's minimal image): every vendor
/// install downloads with it, and without it the repository checks read as "no packages".
#[cfg(any(target_os = "linux", test))]
pub const NEED_CURL: &str = r"command -v curl >/dev/null || { apt-get update && apt-get install -y curl ca-certificates; }";

/// gpg, for the vendors' signing keys (Debian's minimal installs have only gpgv), and a private
/// keyring folder for it in the work directory, so root's own keyring is never read or changed.
pub const NEED_GPG: &str = r#"command -v gpg >/dev/null || { apt-get update && apt-get install -y gnupg; }; mkdir -p -m 700 "$d/gnupg""#;

/// Stops the script unless `"$d/key"`, a vendor's signing key (or keyring) just downloaded, holds
/// exactly the keys pinned here (`fingerprints`, 40 hex digits each, their primary keys), no more
/// and no fewer. TLS alone trusted whatever the vendor's server sent the first time; a swapped key
/// would sign the packages apt then installs as root. Runs after [`NEED_GPG`].
pub fn check_keys(fingerprints: &[&str], vendor: &str) -> String {
    let want = expected(fingerprints);
    format!(
        r#"k="$(gpg --homedir "$d/gnupg" --batch --show-keys --with-colons "$d/key" | awk -F: '$1=="pub"{{p=1}} $1=="fpr"&&p{{print $10; p=0}}' | LC_ALL=C sort | paste -s -d ' ' -)"; [ "$k" = "{want}" ] || {{ echo "{vendor}'s signing key isn't the expected one (fingerprint {want}, got: $k). Nothing was installed." >&2; exit 1; }}"#
    )
}

/// The primary keys' fingerprints [`check_keys`] reads from a key file, as it prints them: sorted,
/// one space apart.
fn expected(fingerprints: &[&str]) -> String {
    let mut sorted = fingerprints.to_vec();
    sorted.sort_unstable();
    sorted.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_keyring_is_checked_for_every_pinned_key_in_the_order_the_check_reads_them() {
        assert_eq!(expected(&["827C", "44C6"]), "44C6 827C");
        assert_eq!(expected(&["9DC8"]), "9DC8");
        let check = check_keys(&["827C", "44C6"], "Kali");
        assert!(check.contains(r#"[ "$k" = "44C6 827C" ]"#), "{check}");
        assert!(check.contains(r#"--homedir "$d/gnupg""#) && check.contains("LC_ALL=C sort"), "{check}");
        assert!(crate::platform::sh_parses(&format!("{PRIVATE_WORKDIR}; {NEED_GPG}; {check}")), "not valid sh: {check}");
    }

    /// `check_keys(pinned)` run against a key file gpg lists as `listing` (gpg stood in for by a
    /// shell function): whether the script went on, and what it said.
    #[cfg(unix)]
    fn checked(pinned: &[&str], listing: &str) -> (bool, String) {
        let script = format!("d=/nonexistent; gpg() {{ printf '%s' '{listing}'; }}; {}; echo went on", check_keys(pinned, "Kali's archive"));
        let out = std::process::Command::new("sh").arg("-c").arg(script).output().unwrap();
        (String::from_utf8_lossy(&out.stdout).contains("went on"), String::from_utf8_lossy(&out.stderr).into_owned())
    }

    #[cfg(unix)]
    #[test]
    fn only_a_keyring_of_exactly_the_pinned_keys_goes_on() {
        // gpg --show-keys --with-colons: each primary key's fingerprint follows its `pub` line, a
        // subkey's its `sub` line (never counted).
        let both = "pub:-:4096:1:ED65462EC8D5E4C5:::::\nfpr:::::::::827C:\nuid:-::::::::Kali 2025:\npub:-:4096:1:ED444FF07D8D0BF6:::::\nfpr:::::::::44C6:\nsub:-:4096:1:A8373E18FC0D0DCB:::::\nfpr:::::::::0D76:\n";
        assert!(checked(&["827C", "44C6"], both).0, "both pinned keys, in any order");
        let (went_on, said) = checked(&["827C"], both);
        assert!(!went_on && said.contains("isn't the expected one (fingerprint 827C, got: 44C6 827C)"), "{said}");
        let (went_on, said) = checked(&["827C", "44C6"], "pub:-:4096:1:X:::::\nfpr:::::::::827C:\n");
        assert!(!went_on && said.contains("got: 827C)"), "one missing: {said}");
        assert!(!checked(&["827C"], "<html>captive portal</html>").0);
    }
}
