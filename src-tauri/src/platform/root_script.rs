//! Pieces of the shell scripts run as root that download something and trust it only once it is
//! checked: a fresh private work directory, gpg with a keyring of its own there, and the check of
//! a downloaded signing key against the fingerprints pinned for it. The vendors' apt repositories
//! a Linux machine sets up (`machine::tools::scripts`) and Kali's archive a Debian attack VM is
//! made Kali from (`runtime::attack_box`) go through the same check.

/// [`PRIVATE_WORKDIR`]'s text, which [`PRELUDE`] starts with too.
macro_rules! private_workdir {
    () => {
        r#"set -e; d="$(mktemp -d)"; trap 'rm -rf "$d"' EXIT; printf 'DPkg::Lock::Timeout "600";\n' >"$d/apt.conf"; export APT_CONFIG="$d/apt.conf""#
    };
}

/// The start of every script run as root that downloads something: stop at the first failure,
/// and work in a fresh private directory `$d` (`mktemp -d`, mode 0700, removed on exit). A fixed
/// path such as `/tmp/aws` could be prepared by any local user beforehand and then run as root.
/// apt waits up to ten minutes for the dpkg lock there (`APT_CONFIG`, a file in `$d`): a fresh
/// Debian runs unattended-upgrades at boot, and the guided setup's Vagrant install met its lock
/// ("Could not get lock /var/lib/dpkg/lock-frontend. It is held by process … (unattended-upgr)",
/// exit 100) and failed instead of waiting.
pub const PRIVATE_WORKDIR: &str = private_workdir!();

/// A shell script run as root (pkexec on Linux). The only way to make one is [`RootScript::new`],
/// which starts it with [`PRELUDE`]: no root script can miss stopping at its first failure, its
/// private work directory, apt's wait for the dpkg lock, or the C locale its output is read in.
/// The step that runs a script as root takes this type, never a `String`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RootScript(String);

/// What every [`RootScript`] starts with: [`PRIVATE_WORKDIR`] (`set -e`, `$d`, apt's lock wait),
/// then the C locale, so a tool's output the script reads (`apt-cache policy`'s `Candidate:`) is
/// the same words on every system: pkexec keeps the player's `LANG`, and apt translates its labels.
pub const PRELUDE: &str = concat!(private_workdir!(), "; export LC_ALL=C");

impl RootScript {
    /// `body` after [`PRELUDE`]: its commands run in order, the first that fails stops the script
    /// with its status (a command that may fail says so itself: `|| true`, `if`).
    pub fn new(body: impl AsRef<str>) -> Self {
        Self(format!("{PRELUDE}; {}", body.as_ref()))
    }

    /// The script, as `sh -c` takes it.
    pub fn into_string(self) -> String {
        self.0
    }

    /// The script, as `sh -c` takes it, with `first` run right after [`PRELUDE`], before its body
    /// (a root step's proxy settings read from their file): still stopping at its first failure.
    pub fn with_after_prelude(self, first: &str) -> String {
        let body = self.0.strip_prefix(PRELUDE).unwrap_or(&self.0);
        format!("{PRELUDE}; {first}{body}")
    }
}

impl std::ops::Deref for RootScript {
    type Target = str;
    fn deref(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for RootScript {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// curl, installed first where a bare system has none (Debian's minimal image): every vendor
/// install downloads with it, and without it the repository checks read as "no packages".
/// Through apt where it is, else dnf (Fedora and the distributions built on it).
#[cfg(any(target_os = "linux", test))]
pub const NEED_CURL: &str = r"command -v curl >/dev/null || { if command -v apt-get >/dev/null; then apt-get update; apt-get install -y curl ca-certificates; else dnf install -y curl ca-certificates; fi; }";

/// unzip, for Amazon's AWS CLI bundle, through apt where it is, else dnf.
#[cfg(any(target_os = "linux", test))]
pub const NEED_UNZIP: &str =
    r"command -v unzip >/dev/null || { if command -v apt-get >/dev/null; then apt-get update; apt-get install -y unzip; else dnf install -y unzip; fi; }";

/// gpg, for the vendors' signing keys (Debian's minimal installs have only gpgv), and a private
/// keyring folder for it in the work directory, so root's own keyring is never read or changed.
pub const NEED_GPG: &str = r#"command -v gpg >/dev/null || { if command -v apt-get >/dev/null; then apt-get update; apt-get install -y gnupg; else dnf install -y gnupg2; fi; }; mkdir -p -m 700 "$d/gnupg""#;

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
    #[test]
    fn the_private_workdir_makes_apt_wait_for_the_dpkg_lock() {
        // Regression: a fresh Debian's unattended-upgrades held the dpkg lock and the guided
        // setup's apt-get failed with exit 100 instead of waiting.
        assert!(
            PRIVATE_WORKDIR.contains(r#"DPkg::Lock::Timeout "600";"#) && PRIVATE_WORKDIR.contains(r#"export APT_CONFIG="$d/apt.conf""#),
            "{PRIVATE_WORKDIR}"
        );
        assert!(crate::platform::sh_parses(PRIVATE_WORKDIR), "not valid sh: {PRIVATE_WORKDIR}");
        #[cfg(unix)]
        {
            let out = std::process::Command::new("sh").arg("-c").arg(format!("{PRIVATE_WORKDIR}; cat \"$APT_CONFIG\"")).output().unwrap();
            assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), r#"DPkg::Lock::Timeout "600";"#);
        }
    }

    use super::*;

    /// `script` run by `sh` with the player's French locale, as pkexec hands it on: its exit
    /// code and what it printed.
    #[cfg(unix)]
    fn ran(script: &RootScript) -> (Option<i32>, String) {
        let out = std::process::Command::new("sh").arg("-c").arg(&**script).env("LANG", "fr_FR.UTF-8").env("LC_ALL", "fr_FR.UTF-8").output().unwrap();
        (out.status.code(), String::from_utf8_lossy(&out.stdout).into_owned())
    }

    #[test]
    fn what_runs_after_the_prelude_comes_before_the_body() {
        let script = RootScript::new("echo body").with_after_prelude("echo first");
        assert_eq!(script, format!("{PRELUDE}; echo first; echo body"));
    }

    #[test]
    fn a_root_script_is_its_body_after_the_prelude() {
        let script = RootScript::new("apt-get install -y qemu-system");
        assert_eq!(&*script, format!("{PRELUDE}; apt-get install -y qemu-system"));
        assert!(PRELUDE.starts_with(PRIVATE_WORKDIR), "{PRELUDE}");
        assert!(crate::platform::sh_parses(&script), "not valid sh: {script}");
    }

    #[cfg(unix)]
    #[test]
    fn a_root_script_stops_at_its_first_failing_command_with_its_status() {
        // Regression: an install chain whose last command (a group change) succeeded made a failed
        // `apt-get install` read as done.
        let (code, said) = ran(&RootScript::new("echo before; sh -c 'exit 100'; echo after"));
        assert_eq!(code, Some(100));
        assert_eq!(said.trim(), "before");
        // A command allowed to fail says so in the body.
        assert_eq!(ran(&RootScript::new("false || true; echo after")), (Some(0), "after\n".to_owned()));
    }

    #[cfg(unix)]
    #[test]
    fn a_root_script_reads_tools_in_the_c_locale_whatever_the_players() {
        // Regression: `apt-cache policy` printed `Candidat :` under a French LANG, and a grep for
        // `Candidate:` never matched.
        assert_eq!(ran(&RootScript::new(r#"echo "$LC_ALL""#)), (Some(0), "C\n".to_owned()));
    }

    #[test]
    fn curl_and_gpg_come_through_apt_where_it_is_else_dnf() {
        for need in [NEED_CURL, NEED_GPG, NEED_UNZIP] {
            assert!(need.contains("if command -v apt-get >/dev/null; then apt-get") && need.contains("else dnf install -y"), "{need}");
            assert!(crate::platform::sh_parses(need), "not valid sh: {need}");
        }
        assert!(NEED_GPG.contains("dnf install -y gnupg2"), "Fedora's package is gnupg2");
    }

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
