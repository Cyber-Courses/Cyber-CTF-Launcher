//! The shell scripts behind some installs: macOS casks opened in their own installer, and the
//! vendors' apt repositories on Linux. Every repository's signing key is checked against its
//! fingerprint, pinned here, before apt trusts it; no vendor install script runs as root.

#[cfg(target_os = "macos")]
use super::steps::Step;

// Download the installer with brew (no admin needed), then open it so the tool's own
// native installer runs and the user finishes there (trusted OS prompts, no custom dialog).
// A disk image is mounted quietly and its .pkg opened in Installer, which comes to the front;
// only an app to drag into Applications (Docker Desktop, UTM) is shown in Finder, brought
// forward. Either way the log says where to finish: a window behind the app went unnoticed.
// `brew fetch` doesn't update Homebrew, and an old Homebrew can crash reading today's cask
// data (`undefined method 'first' for nil` in api/cask.rb), so a failed fetch updates
// Homebrew and tries once more.
#[cfg(target_os = "macos")]
pub fn fetch_and_open(brew: &str, cask: &str, name: &str) -> Step {
    let script = format!(
        r#"set -e
if ! '{brew}' fetch --cask {cask}; then
  echo "Homebrew couldn't download {name}. Updating Homebrew and trying again (this can take a few minutes)…"
  '{brew}' update || true
  if ! '{brew}' fetch --cask {cask}; then
    echo "Homebrew still can't download {name}. Run 'brew update' in Terminal, or install {name} from its website, then try again." >&2
    exit 1
  fi
fi
f="$('{brew}' --cache --cask {cask})"
case "$f" in
  *.dmg)
    m="$(hdiutil attach -nobrowse -noautoopen "$f" | awk -F'\t' '/\/Volumes\//{{print $NF; exit}}')"
    p="$(find "$m" -maxdepth 1 -name '*.pkg' | head -n 1)"
    if [ -n "$p" ]; then
      open "$p"
      echo "The {name} installer is open in its own window. Finish it there; this step updates by itself."
    else
      open "$m"
      osascript -e 'tell application "Finder" to activate' >/dev/null 2>&1 || true
      echo "A Finder window with {name} is open. Drag it into Applications, then open it once; this step updates by itself."
    fi
    ;;
  *)
    open "$f"
    echo "The {name} installer is open in its own window. Finish it there; this step updates by itself."
    ;;
esac"#
    );
    Step { program: "sh".into(), args: vec!["-c".into(), script], note: Some(format!("Downloading {name}…")) }
}

#[cfg(any(target_os = "linux", test))]
use crate::platform::root_script::{NEED_CURL, NEED_GPG, PRIVATE_WORKDIR, check_keys};

/// The distro's codename as the vendors' repos name it (`$codename`): an Ubuntu derivative like
/// Mint uses its Ubuntu base's.
#[cfg(any(target_os = "linux", test))]
const CODENAME: &str =
    r#"codename="$(. /etc/os-release && echo "${UBUNTU_CODENAME:-$VERSION_CODENAME}")"; [ -n "$codename" ] || codename="$(lsb_release -cs)""#;

/// A vendor's apt repository: where its packages and its signing key are, the key's pinned
/// fingerprint, and the name its keyring and its source list take here.
#[cfg(any(target_os = "linux", test))]
struct VendorRepo {
    /// Who signs it, as a refusal names them.
    vendor: &'static str,
    /// Its signing key's fingerprint, pinned (40 hex digits).
    fingerprint: &'static str,
    /// Where its signing key is (a shell word: it may use `$repo`).
    key_url: &'static str,
    /// The repository (a shell word: it may use `$dist`).
    repo: &'static str,
    /// Its suite: `$codename` for one per distro release.
    suite: &'static str,
    component: &'static str,
    /// The keyring's and the source list's name: `/etc/apt/keyrings/<name>.gpg`,
    /// `/etc/apt/sources.list.d/<name>.list`.
    name: &'static str,
    /// Where to install from instead when the repository has no packages for this release; none
    /// for a suite every release shares, or a release the caller has checked already.
    by_hand: Option<&'static str>,
}

/// `release_served <repo> <suite>`: whether apt repository `<repo>` has packages for `<suite>`,
/// by its `Release` file. A 2xx answer is yes and a 404 no; anything else (no answer at all: a
/// DNS failure, a proxy, a timeout; or a server error) stops the script saying the repository
/// couldn't be reached, so a network fault is never taken for a release the vendor doesn't serve.
#[cfg(any(target_os = "linux", test))]
const RELEASE_SERVED: &str = r#"release_served() { served=$(curl -s -o /dev/null -w '%{http_code}' "$1/dists/$2/Release" || true); case "$served" in 2??) return 0 ;; 404) return 1 ;; *) echo "Couldn't reach $1 (HTTP ${served:-none}). Check this machine's internet access or proxy, then try again." >&2; exit 1 ;; esac; }"#;

/// curl, then [`RELEASE_SERVED`]: what a script needs before it asks a vendor's repository
/// anything.
#[cfg(any(target_os = "linux", test))]
fn fetch_prelude() -> String {
    format!("{NEED_CURL}; {RELEASE_SERVED}")
}

/// [`apt_repo_setup`] after [`fetch_prelude`]: a vendor's apt repository on a machine that may
/// lack curl.
#[cfg(any(target_os = "linux", test))]
fn apt_repo_script(r: &VendorRepo) -> String {
    format!("{}; {}", fetch_prelude(), apt_repo_setup(r))
}

/// Sets up `r` for apt, the same way for every vendor: the suite checked when releases have their
/// own ([`RELEASE_SERVED`]), the key downloaded, checked against its fingerprint and only then
/// made a keyring under `/etc/apt/keyrings`, the source restricted to this machine's architecture
/// and to that key, and apt's lists updated. `--batch --yes`: a second run (after a partial one)
/// overwrites the keyring instead of asking on a terminal it doesn't have. Runs in
/// [`PRIVATE_WORKDIR`], after [`fetch_prelude`].
#[cfg(any(target_os = "linux", test))]
fn apt_repo_setup(r: &VendorRepo) -> String {
    let VendorRepo { vendor, fingerprint, key_url, repo, suite, component, name, by_hand } = r;
    let suite_check = by_hand.map(|url| {
        format!(r#"; release_served "$repo" "{suite}" || {{ echo "{vendor} has no packages for {suite} yet. Install it from {url}" >&2; exit 1; }}"#)
    });
    format!(
        r#"repo="{repo}"{suite_check}; {NEED_GPG}; curl -fsSL "{key_url}" -o "$d/key"; {check}; install -m 0755 -d /etc/apt/keyrings; gpg --batch --yes --dearmor --homedir "$d/gnupg" -o /etc/apt/keyrings/{name}.gpg "$d/key"; echo "deb [arch=$(dpkg --print-architecture) signed-by=/etc/apt/keyrings/{name}.gpg] $repo {suite} {component}" > /etc/apt/sources.list.d/{name}.list; apt-get update"#,
        suite_check = suite_check.unwrap_or_default(),
        check = check_keys(&[fingerprint], vendor),
    )
}

/// HashiCorp's package signing key (`security+packaging@hashicorp.com`, created 2026-09-09,
/// expires 2031-09-08), the one apt.releases.hashicorp.com serves at `/gpg` and signs its
/// `InRelease` files with; keys.openpgp.org and keyserver.ubuntu.com have the same. When HashiCorp
/// rotates it, installs stop with "isn't the expected one" until this is updated.
#[cfg(any(target_os = "linux", test))]
const HASHICORP: VendorRepo = VendorRepo {
    vendor: "HashiCorp",
    fingerprint: "D55C0D1AC78A8D8126CB631CFC9CA96ACA026560",
    key_url: "https://apt.releases.hashicorp.com/gpg",
    repo: "https://apt.releases.hashicorp.com",
    suite: "$codename",
    component: "main",
    name: "hashicorp",
    by_hand: Some("https://developer.hashicorp.com/terraform/install"),
};

/// HashiCorp's apt repo, then terraform.
#[cfg(any(target_os = "linux", test))]
pub fn terraform_script() -> String {
    format!("{PRIVATE_WORKDIR}; {CODENAME}; {}; apt-get install -y terraform", apt_repo_script(&HASHICORP))
}

/// HashiCorp's apt repo, then Vagrant: the distributions' own package is older than labs need
/// (Debian 13 ships 2.3.8; labs need 2.4.0 or later), so a setup that installed it left a machine
/// that refused every VM lab.
#[cfg(any(target_os = "linux", test))]
pub fn vagrant_script() -> String {
    const VAGRANT: VendorRepo = VendorRepo { by_hand: Some("https://developer.hashicorp.com/vagrant/install"), ..HASHICORP };
    format!("{PRIVATE_WORKDIR}; {CODENAME}; {}; apt-get install -y vagrant", apt_repo_script(&VAGRANT))
}

/// HashiCorp's apt repo, then the Vagrant VMware Utility, whose package generates its
/// certificates and enables and starts its service. `--reinstall`: an install that is there
/// already has its package configured again, which restarts a service that had stopped.
#[cfg(any(target_os = "linux", test))]
pub fn vmware_utility_script() -> String {
    // HashiCorp's repository, but a release it doesn't serve yet is sent to the utility's own page.
    let repo = VendorRepo { by_hand: Some("https://developer.hashicorp.com/vagrant/install/vmware"), ..HASHICORP };
    format!("{PRIVATE_WORKDIR}; {CODENAME}; {}; apt-get install -y --reinstall vagrant-vmware-utility", apt_repo_script(&repo))
}

/// Oracle's VirtualBox archive signing key (`info@virtualbox.org`, "Oracle Corporation (VirtualBox
/// archive signing key)"), the one www.virtualbox.org serves as `oracle_vbox_2016.asc` and
/// download.virtualbox.org signs its `InRelease` files with.
#[cfg(any(target_os = "linux", test))]
const ORACLE: VendorRepo = VendorRepo {
    vendor: "Oracle",
    fingerprint: "B9F8D658297AF3EFC18D5CDFA2F683C52980AECF",
    key_url: "https://www.virtualbox.org/download/oracle_vbox_2016.asc",
    repo: "https://download.virtualbox.org/virtualbox/debian",
    suite: "$codename",
    component: "contrib",
    name: "virtualbox",
    by_hand: Some("https://www.virtualbox.org/wiki/Linux_Downloads"),
};

/// The distro's own `virtualbox` when it has one (Ubuntu), else Oracle's apt repo and its newest
/// `virtualbox-X.Y` (Debian ships none in main, so `apt-get install virtualbox` failed).
#[cfg(any(target_os = "linux", test))]
pub fn virtualbox_script() -> String {
    format!(
        r#"{PRIVATE_WORKDIR}; apt-get update; if apt-cache policy virtualbox 2>/dev/null | grep -q 'Candidate: [0-9]'; then apt-get install -y virtualbox; exit 0; fi; {CODENAME}; {}; pkg="$(apt-cache search --names-only '^virtualbox-[0-9]+\.[0-9]+$' | awk '{{print $1}}' | sort -V | tail -n 1)"; [ -n "$pkg" ] || {{ echo "No VirtualBox package found in Oracle's repo. Install it from https://www.virtualbox.org/wiki/Linux_Downloads" >&2; exit 1; }}; apt-get install -y "$pkg""#,
        apt_repo_script(&ORACLE)
    )
}

/// Docker's release key (`docker@docker.com`, Docker Release (CE deb)), the one download.docker.com
/// serves at `/linux/{ubuntu,debian,raspbian}/gpg` and signs its `InRelease` files with (from its
/// signing subkey D3306A018370199E527AE7997EA0A9C3F273FCD8); keyserver.ubuntu.com has the same.
/// Its repository is one per distro (`$dist`, see [`DOCKER_DIST`]), whose release is checked there
/// (a release it doesn't serve gets the distribution's own Docker), so not again here.
#[cfg(any(target_os = "linux", test))]
const DOCKER: VendorRepo = VendorRepo {
    vendor: "Docker",
    fingerprint: "9DC858229FC7DD38854AE2D88D81803C0EBFCD88",
    key_url: "$repo/gpg",
    repo: "https://download.docker.com/linux/$dist",
    suite: "$codename",
    component: "stable",
    name: "docker",
    by_hand: None,
};

/// Docker Engine, then the player into the `docker` group so the CLI can reach the daemon without
/// root (pkexec runs as root, so `$USER` there is root: the name comes from the app). The group
/// applies from the next login.
///
/// On Ubuntu, Debian and Raspbian, and their derivatives (on the Ubuntu or Debian release they are
/// built on, when they say it), it sets up Docker's apt repository, the way Docker documents it,
/// with Docker's key checked against its pinned fingerprint. Where that repository has no packages
/// for the release (Kali's rolling one, Devuan's, a Debian or Ubuntu newer than Docker serves yet:
/// its `Release` file answers 404), it installs the distribution's own Docker (`docker.io` and its Compose), signed by the
/// distribution's own keys. When download.docker.com can't be reached, it stops and says so. Elsewhere
/// it stops and says where to get Docker: Docker's convenience
/// script (get.docker.com) can't be held to a key (it fetches the key and installs what it signed
/// in one go), so it never runs as root from here.
#[cfg(any(target_os = "linux", test))]
pub fn docker_script(user: Option<&str>) -> String {
    use std::fmt::Write as _;
    let mut s = format!(
        "{PRIVATE_WORKDIR}; {}; {DOCKER_DIST}; [ -n \"$dist\" ] || {{ echo 'Cyber CTF installs Docker on Debian, Ubuntu and the distributions built on them. Install Docker Engine from your distribution or https://docs.docker.com/engine/install/, then check again.' >&2; exit 1; }}; if [ -n \"$own\" ]; then {DISTRIBUTION_DOCKER}; else {}; apt-get install -y docker-ce docker-ce-cli containerd.io docker-buildx-plugin docker-compose-plugin; fi",
        fetch_prelude(),
        apt_repo_setup(&DOCKER)
    );
    if let Some(u) = user.filter(|u| valid_user(u)) {
        let _ = write!(s, "; usermod -aG docker {u}; echo 'Added {u} to the docker group: log out and back in (or restart) for it to apply.'");
    }
    s
}

/// Which of Docker's apt repositories fits this system (`$dist`, empty when none does) and its
/// codename there; `$own` set when that repository has no packages for it ([`RELEASE_SERVED`]: a
/// 404, never an unreachable server, which stops the script), so the distribution's own Docker is
/// installed. A derivative is known by `ID_LIKE`, on its base's release: Ubuntu's
/// (`UBUNTU_CODENAME`, Mint), Debian's (`DEBIAN_CODENAME`, LMDE), else its own (Kali, Devuan),
/// which Docker doesn't serve. Raspbian Trixie has none of Docker's: Debian's serves it (as
/// get.docker.com does).
#[cfg(any(target_os = "linux", test))]
const DOCKER_DIST: &str = r#". /etc/os-release; dist=""; own=""; codename="${VERSION_CODENAME:-}"; case "$ID" in ubuntu|debian|raspbian) dist="$ID";; *) case " ${ID_LIKE:-} " in *" ubuntu "*) dist=ubuntu;; *" debian "*) dist=debian; codename="${DEBIAN_CODENAME:-$codename}";; esac;; esac; [ "$dist" != ubuntu ] || codename="${UBUNTU_CODENAME:-$codename}"; [ "$dist$codename" != raspbiantrixie ] || dist=debian; command -v apt-get >/dev/null || dist=""; if [ -n "$dist" ] && ! release_served "https://download.docker.com/linux/$dist" "$codename"; then own=1; fi"#;

/// The distribution's own Docker: `docker.io`, and its Compose plugin (`docker-compose-v2` on
/// Ubuntu, `docker-compose` on Debian and Kali), from the distribution's archive.
#[cfg(any(target_os = "linux", test))]
const DISTRIBUTION_DOCKER: &str = r#"echo "Docker's apt repository has no packages for $dist $codename: installing the distribution's own Docker (docker.io)."; apt-get update; apt-get install -y docker.io; if apt-cache show docker-compose-v2 >/dev/null 2>&1; then apt-get install -y docker-compose-v2; else apt-get install -y docker-compose; fi"#;

/// The AWS CLI team's signing key (`aws-cli@amazon.com`, created 2019-09-18, expires 2027-07-01 as
/// published), as AWS publishes it in "Installing or updating to the latest version of the AWS
/// CLI" (Linux, "Verify the integrity and authenticity of the downloaded zip file"). An expired
/// copy still verifies older signatures; refresh it from that page when AWS extends it.
#[cfg(any(target_os = "linux", test))]
const AWS_CLI_KEY: &str = include_str!("keys/aws-cli.asc");
/// Its fingerprint, as that page gives it (FB5D B77F D5C1 18B8 0511 ADA8 A631 0ACC 4672 475C).
#[cfg(any(target_os = "linux", test))]
pub const AWS_CLI_KEY_FINGERPRINT: &str = "FB5DB77FD5C118B80511ADA8A6310ACC4672475C";

/// AWS CLI v2 from Amazon's official bundle, unpacked and installed from a private directory.
/// The distro `awscli` package is v1, which has no `aws login` (the app needs v2 >= 2.32).
/// `uname -m` is `x86_64` or `aarch64`, matching the URLs. The bundle must carry a valid signature
/// (its `.sig`) by AWS's key, embedded here ([`AWS_CLI_KEY`]): gpgv, with only that key, must
/// report it by its fingerprint. Then `unzip -t` checks the archive is whole before anything
/// in it runs.
#[cfg(any(target_os = "linux", test))]
pub fn awscli_script() -> String {
    format!(
        r#"{PRIVATE_WORKDIR}; {NEED_CURL}; url="https://awscli.amazonaws.com/awscli-exe-linux-$(uname -m).zip"; curl -fsSL "$url" -o "$d/awscliv2.zip"; curl -fsSL "$url.sig" -o "$d/awscliv2.zip.sig"; {}; command -v unzip >/dev/null || {{ apt-get update && apt-get install -y unzip; }}; unzip -tq "$d/awscliv2.zip" >/dev/null; unzip -oq "$d/awscliv2.zip" -d "$d"; "$d/aws/install" --update"#,
        aws_verify()
    )
}

/// Stops the script unless `"$d/awscliv2.zip.sig"` is a valid signature of `"$d/awscliv2.zip"`
/// by [`AWS_CLI_KEY`].
#[cfg(any(target_os = "linux", test))]
fn aws_verify() -> String {
    let key = AWS_CLI_KEY.trim_end();
    format!(
        r#"{NEED_GPG}; cat > "$d/aws-cli.asc" <<'AWS_CLI_KEY'
{key}
AWS_CLI_KEY
gpg --batch --yes --dearmor --homedir "$d/gnupg" -o "$d/aws-cli.gpg" "$d/aws-cli.asc"; gpgv --homedir "$d/gnupg" --status-fd 1 --keyring "$d/aws-cli.gpg" "$d/awscliv2.zip.sig" "$d/awscliv2.zip" 2>/dev/null | grep -q '^\[GNUPG:\] VALIDSIG {AWS_CLI_KEY_FINGERPRINT} ' || {{ echo "The AWS CLI download isn't signed by AWS's key ({AWS_CLI_KEY_FINGERPRINT}). Nothing was installed." >&2; exit 1; }}"#
    )
}

/// Microsoft's package signing key (`gpgsecurity@microsoft.com`, "Microsoft (Release signing)"),
/// the one packages.microsoft.com serves at `/keys/microsoft.asc` and signs the azure-cli
/// repository's `InRelease` files with (its key id EB3E94ADBE1229CF).
#[cfg(any(target_os = "linux", test))]
const MICROSOFT: VendorRepo = VendorRepo {
    vendor: "Microsoft",
    fingerprint: "BC528686B50D79E339D3721CEB3E94ADBE1229CF",
    key_url: "https://packages.microsoft.com/keys/microsoft.asc",
    repo: "https://packages.microsoft.com/repos/azure-cli",
    suite: "$codename",
    component: "main",
    name: "azure-cli",
    by_hand: Some("https://learn.microsoft.com/cli/azure/install-azure-cli-linux"),
};

/// Microsoft's azure-cli apt repository for this distro's codename, then `azure-cli`: what
/// Microsoft's install script (aka.ms/InstallAzureCLIDeb) sets up, without running that script as
/// root.
#[cfg(any(target_os = "linux", test))]
pub fn azurecli_script() -> String {
    format!("{PRIVATE_WORKDIR}; {CODENAME}; {}; apt-get install -y azure-cli", apt_repo_script(&MICROSOFT))
}

/// Google Cloud's package signing key ("Artifact Registry Repository Signer"), the one
/// packages.cloud.google.com serves at `/apt/doc/apt-key.gpg` and signs the cloud-sdk
/// repository's `InRelease` files with. One suite for every release.
#[cfg(any(target_os = "linux", test))]
const GOOGLE: VendorRepo = VendorRepo {
    vendor: "Google Cloud",
    fingerprint: "35BAA0B33E9EB396F59CA838C0BA5CE6DC6315A3",
    key_url: "https://packages.cloud.google.com/apt/doc/apt-key.gpg",
    repo: "https://packages.cloud.google.com/apt",
    suite: "cloud-sdk",
    component: "main",
    name: "google-cloud-sdk",
    by_hand: None,
};

/// Google Cloud's apt repository, then `google-cloud-cli` (gcloud), system-wide: what Google
/// documents for Debian and Ubuntu, in place of its install script.
#[cfg(any(target_os = "linux", test))]
pub fn gcloud_script() -> String {
    format!("{PRIVATE_WORKDIR}; {}; apt-get install -y google-cloud-cli", apt_repo_script(&GOOGLE))
}

/// libvirt, with what `vagrant plugin install vagrant-libvirt` compiles against: libvirt's headers
/// (`libvirt-dev`), a C compiler and make (`build-essential`), `pkg-config`, and Ruby's headers
/// for a distribution's Vagrant (which builds on the system Ruby). Without them the plugin's
/// install fails building its native extension. Then the player joins the `libvirt` group (and
/// `kvm`, where the distribution has one), so Vagrant reaches `qemu:///system` and `/dev/kvm`
/// without root; the groups apply from the next login.
#[cfg(any(target_os = "linux", test))]
pub fn libvirt_script(user: Option<&str>) -> String {
    use std::fmt::Write as _;
    // virtiofsd: libvirt's shared folders (the attack box's /shared) need it, and it is its own
    // package from Debian 12 and Ubuntu 23.10 on (QEMU's before); a release without it skips it.
    let mut s = "apt-get update && apt-get install -y libvirt-daemon-system libvirt-dev build-essential pkg-config ruby-dev virt-manager \
                 && if apt-cache show virtiofsd >/dev/null 2>&1; then apt-get install -y virtiofsd; fi"
        .to_string();
    if let Some(u) = user.filter(|u| valid_user(u)) {
        let _ = write!(
            s,
            "; usermod -aG libvirt {u}; if getent group kvm >/dev/null; then usermod -aG kvm {u}; fi; echo 'Added {u} to the libvirt group: log out and back in (or restart) for it to apply.'"
        );
    }
    s
}

/// A login name safe to put in a shell script unquoted.
#[cfg(any(target_os = "linux", test))]
fn valid_user(u: &str) -> bool {
    !u.is_empty()
        && u.len() <= 32
        && u.chars().next().is_some_and(|c| c.is_ascii_lowercase() || c == '_')
        && u.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn docker_install_adds_the_player_to_the_docker_group() {
        let s = docker_script(Some("florianamette"));
        assert!(s.contains("usermod -aG docker florianamette"), "{s}");
        // A name that isn't a plain login name never reaches the script.
        assert!(!docker_script(Some("a; rm -rf /")).contains("usermod"));
        assert!(!docker_script(None).contains("usermod"));
        assert!(valid_user("_svc-1.x") && !valid_user("Root") && !valid_user(""));
    }

    #[test]
    fn libvirt_install_brings_what_the_vagrant_libvirt_plugin_compiles_against() {
        // Regression: libvirt alone, and `vagrant plugin install vagrant-libvirt` then failed
        // building its native extension (no libvirt headers, no compiler).
        let s = libvirt_script(None);
        for package in ["libvirt-daemon-system", "libvirt-dev", "build-essential", "pkg-config", "ruby-dev"] {
            assert!(s.split_whitespace().any(|w| w == package), "{package}: {s}");
        }
        assert!(!s.contains("usermod"));
        let s = libvirt_script(Some("ada"));
        assert!(s.contains("usermod -aG libvirt ada") && s.contains("usermod -aG kvm ada"), "{s}");
        assert!(!libvirt_script(Some("a; rm -rf /")).contains("usermod"));
        assert!(crate::platform::sh_parses(&s), "not valid sh: {s}");
    }

    #[test]
    fn libvirt_install_brings_virtiofsd_for_the_shared_folder_where_the_release_has_it() {
        // Regression: libvirt alone, and an attack box with the shared folder failed to start on
        // libvirt ("Unable to find a satisfying virtiofsd").
        let s = libvirt_script(None);
        assert!(s.contains("if apt-cache show virtiofsd >/dev/null 2>&1; then apt-get install -y virtiofsd; fi"), "{s}");
        assert!(crate::platform::sh_parses(&s), "not valid sh: {s}");
    }

    #[test]
    fn login_names_at_the_edges_of_what_goes_unquoted_into_the_script() {
        // 32 characters is the longest a login name can be; one more, a capital, a digit or a
        // dash first, a space or non-ASCII anywhere: not a name, so no `usermod` line.
        assert!(valid_user(&"a".repeat(32)) && !valid_user(&"a".repeat(33)));
        assert!(valid_user("a") && valid_user("_") && valid_user("a-") && valid_user("a.b"));
        for bad in ["1a", "-a", "A", "ab c", "caf\u{e9}", "a;b", "a$b", "a\nb"] {
            assert!(!valid_user(bad), "{bad:?}");
            assert!(!docker_script(Some(bad)).contains("usermod"), "{bad:?}");
        }
        // The group line names the player once, after the install, and the script stays one line.
        let s = docker_script(Some("ada"));
        assert_eq!(s.matches("usermod -aG docker ada").count(), 1);
        assert!(!s.contains('\n'));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_failed_cask_fetch_updates_homebrew_and_retries_once() {
        // A Homebrew whose fetch always fails (an old one crashing on the cask data).
        let dir = std::env::temp_dir().join(format!("cyberctf-brew-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let brew = dir.join("brew");
        let log = dir.join("calls");
        std::fs::write(&brew, format!("#!/bin/sh\necho \"$1\" >> '{}'\n[ \"$1\" = update ]\n", log.display())).unwrap();
        std::fs::set_permissions(&brew, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();

        let step = fetch_and_open(brew.to_str().unwrap(), "vagrant", "Vagrant");
        let out = std::process::Command::new(&step.program).args(&step.args).output().unwrap();
        let calls = std::fs::read_to_string(&log).unwrap();
        std::fs::remove_dir_all(&dir).ok();

        assert!(!out.status.success());
        assert_eq!(calls.lines().collect::<Vec<_>>(), ["fetch", "update", "fetch"]);
        assert!(String::from_utf8_lossy(&out.stderr).contains("Run 'brew update' in Terminal"));
    }

    /// Every vendor's apt repository, with the script that sets it up.
    fn repo_scripts() -> [(&'static VendorRepo, String); 7] {
        [
            (&HASHICORP, terraform_script()),
            (&HASHICORP, vagrant_script()),
            (&HASHICORP, vmware_utility_script()),
            (&ORACLE, virtualbox_script()),
            (&DOCKER, docker_script(None)),
            (&MICROSOFT, azurecli_script()),
            (&GOOGLE, gcloud_script()),
        ]
    }

    #[test]
    fn repo_scripts_rerun_without_a_terminal_and_parse() {
        // gpg asked "overwrite?" on a second run, with no tty to answer it.
        for (_, s) in repo_scripts() {
            assert!(s.contains("gpg --batch --yes --dearmor"), "{s}");
            assert!(crate::platform::sh_parses(&s), "not valid sh: {s}");
        }
        assert!(virtualbox_script().contains("download.virtualbox.org/virtualbox/debian"));
        // VMware's utility from HashiCorp's repository, its key pinned like Terraform's; a release
        // the repository doesn't serve yet is sent to the utility's own page.
        let utility = vmware_utility_script();
        assert!(utility.contains(HASHICORP.fingerprint) && utility.contains("apt-get install -y --reinstall vagrant-vmware-utility"), "{utility}");
        assert!(utility.contains("developer.hashicorp.com/vagrant/install/vmware") && !utility.contains("terraform/install"), "{utility}");
    }

    #[test]
    fn every_vendor_repository_is_set_up_the_same_way() {
        // Regression: five copies of one sequence had drifted (keyrings in /usr/share/keyrings or
        // /etc/apt/keyrings, an architecture on some sources and not others).
        for (repo, s) in repo_scripts() {
            let name = repo.name;
            assert!(s.contains(&format!("-o /etc/apt/keyrings/{name}.gpg")), "{s}");
            let source = format!("signed-by=/etc/apt/keyrings/{name}.gpg] $repo {} {}\" > /etc/apt/sources.list.d/{name}.list", repo.suite, repo.component);
            assert!(s.contains(&format!("deb [arch=$(dpkg --print-architecture) {source}")), "{s}");
            assert!(!s.contains("/usr/share/keyrings"), "{s}");
            assert_eq!(s.contains(&format!(r#"release_served "$repo" "{}""#, repo.suite)), repo.by_hand.is_some(), "{s}");
        }
    }

    /// Every install script that runs as root.
    fn root_scripts() -> [String; 8] {
        [
            awscli_script(),
            docker_script(Some("player")),
            terraform_script(),
            vagrant_script(),
            virtualbox_script(),
            azurecli_script(),
            gcloud_script(),
            vmware_utility_script(),
        ]
    }

    #[test]
    fn every_root_script_that_downloads_installs_curl_first() {
        // Regression: Debian's minimal image has no curl; the Vagrant install failed, saying
        // HashiCorp had no packages for trixie, and Docker's repository check read as "not
        // served", so Docker came from the distribution instead.
        for s in root_scripts() {
            if let Some(first) = s.find("curl -") {
                let installed = s.find(NEED_CURL).unwrap_or_else(|| panic!("curl used, never installed: {s}"));
                assert!(installed < first, "curl used before it is installed: {s}");
            }
        }
    }

    /// Runs `script` with [`RELEASE_SERVED`] defined and a `curl` that answers HTTP status `http`:
    /// its stdout and stderr.
    #[cfg(unix)]
    fn with_release_answer(http: &str, script: &str) -> (String, String) {
        let bin = std::env::temp_dir().join(format!("cyberctf-curl-{}-{}", std::process::id(), rand::random::<u32>()));
        std::fs::create_dir_all(&bin).unwrap();
        let fake = bin.join("curl");
        std::fs::write(&fake, format!("#!/bin/sh\nprintf '%s' '{http}'\n")).unwrap();
        std::fs::set_permissions(&fake, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        let out = std::process::Command::new("sh")
            .arg("-c")
            .arg(format!("set -e; {RELEASE_SERVED}; {script}"))
            .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
            .output()
            .unwrap();
        std::fs::remove_dir_all(&bin).unwrap();
        (String::from_utf8_lossy(&out.stdout).into_owned(), String::from_utf8_lossy(&out.stderr).into_owned())
    }

    #[cfg(unix)]
    #[test]
    fn a_repository_check_says_no_packages_only_when_the_release_has_none() {
        // Regression: any curl failure (curl missing, no network) read as "no packages for trixie".
        let s = apt_repo_script(&HASHICORP);
        let snippet = &s[s.find("; release_served \"$repo\"").unwrap() + 2..s.find("; command -v gpg").unwrap()];
        let check = |http: &str| with_release_answer(http, &format!("repo=https://apt.example; codename=trixie; {snippet}; echo went-on"));
        assert_eq!(check("200").0.trim(), "went-on");
        let missing = check("404").1;
        assert!(missing.starts_with("HashiCorp has no packages for trixie yet"), "{missing}");
        let unreachable = check("000").1;
        assert!(unreachable.starts_with("Couldn't reach https://apt.example (HTTP 000)") && !unreachable.contains("no packages"), "{unreachable}");
    }

    #[cfg(unix)]
    #[test]
    fn dockers_repository_check_gives_the_distributions_docker_only_on_a_404() {
        // Regression: any failure of Docker's probe (DNS, a proxy, a timeout) installed docker.io,
        // saying Docker's repository had no packages for the release.
        let dir = std::env::temp_dir().join(format!("cyberctf-docker-probe-{}-{}", std::process::id(), rand::random::<u32>()));
        std::fs::create_dir_all(&dir).unwrap();
        let release = dir.join("os-release");
        std::fs::write(&release, "ID=debian\nVERSION_CODENAME=trixie\n").unwrap();
        let probe = DOCKER_DIST.replace("/etc/os-release", &release.display().to_string()).replace("command -v apt-get >/dev/null", "true");
        let check = |http: &str| with_release_answer(http, &format!("{probe}; echo \"$dist/$codename/$own\""));
        assert_eq!(check("200").0.trim(), "debian/trixie/");
        assert_eq!(check("404").0.trim(), "debian/trixie/1");
        for unreachable in ["000", "407", "503"] {
            let (out, err) = check(unreachable);
            assert!(out.is_empty(), "went on after HTTP {unreachable}: {out}");
            assert!(err.starts_with(&format!("Couldn't reach https://download.docker.com/linux/debian (HTTP {unreachable})")), "{err}");
        }
        // The script asks once, and defines the probe before it asks.
        let s = docker_script(None);
        assert_eq!(s.matches(NEED_CURL).count(), 1, "{s}");
        assert_eq!(s.matches("release_served()").count(), 1, "{s}");
        assert!(s.find("release_served()").unwrap() < s.find("! release_served").unwrap());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn vagrant_names_its_own_install_page() {
        assert!(vagrant_script().contains("https://developer.hashicorp.com/vagrant/install"), "not Terraform's page");
    }

    #[test]
    fn root_scripts_never_use_fixed_temp_paths_or_pipe_downloads_into_a_shell() {
        // Under pkexec the AWS CLI was unzipped into /tmp/aws and /tmp/aws/install run as root:
        // any local user could plant that file first.
        // Every download fails on an HTTP error (`-f`); the one curl that doesn't is the
        // repository check, which reads the status code itself (RELEASE_SERVED).
        const STATUS_PROBE: &str = "s -o /dev/null -w '%{http_code}'";
        assert!(RELEASE_SERVED.contains(&format!("curl -{STATUS_PROBE}")));
        for s in root_scripts() {
            assert!(!s.contains("/tmp"), "fixed temp path: {s}");
            assert!(!s.contains("| sh") && !s.contains("| bash"), "download piped into a shell: {s}");
            for call in s.split("curl -").skip(1) {
                assert!(call.starts_with('f') || call.starts_with(STATUS_PROBE), "a failed download must fail: curl -{call}");
            }
            assert!(s.starts_with("set -e"), "{s}");
            assert!(crate::platform::sh_parses(&s), "not valid sh: {s}");
        }
        for s in root_scripts() {
            assert!(s.contains(r#"d="$(mktemp -d)""#) && s.contains("trap 'rm -rf \"$d\"' EXIT"), "{s}");
        }
        assert!(awscli_script().contains(r#""$d/aws/install" --update"#) && awscli_script().contains("unzip -tq"));
    }

    /// Whether `program` runs here (the gpg checks run for real where gpg is installed).
    #[cfg(unix)]
    fn have(program: &str) -> bool {
        std::process::Command::new(program).arg("--version").output().is_ok_and(|o| o.status.success())
    }

    /// Runs `script` with `sh`, in a fresh directory `$d` holding `files`: its exit success and
    /// its stderr.
    #[cfg(unix)]
    fn run_in_workdir(script: &str, files: &[(&str, &[u8])]) -> (bool, String) {
        let d = std::env::temp_dir().join(format!("cyberctf-script-{}-{}", std::process::id(), rand::random::<u32>()));
        std::fs::create_dir_all(&d).unwrap();
        for (name, body) in files {
            std::fs::write(d.join(name), body).unwrap();
        }
        let out = std::process::Command::new("sh").arg("-c").arg(format!("set -e; d='{}'; {script}", d.display())).output().unwrap();
        let _ = std::fs::remove_dir_all(&d);
        (out.status.success(), String::from_utf8_lossy(&out.stderr).into_owned())
    }

    #[test]
    fn vendor_keys_are_pinned_and_checked_before_apt_trusts_them() {
        let vendors = repo_scripts().map(|(repo, _)| repo.fingerprint);
        for fpr in vendors.into_iter().chain([AWS_CLI_KEY_FINGERPRINT]) {
            assert!(fpr.len() == 40 && fpr.bytes().all(|b| b.is_ascii_digit() || (b'A'..=b'F').contains(&b)), "{fpr}");
        }
        // The key is checked, then installed for apt: never fetched straight into the keyring.
        let check = |s: &str, fpr: &str, install: &str| {
            let checked = s.find(&format!(r#"[ "$k" = "{fpr}" ]"#)).unwrap_or_else(|| panic!("no fingerprint check: {s}"));
            let installed = s.find(install).unwrap_or_else(|| panic!("no install of the key: {s}"));
            assert!(s.find(r#"-o "$d/key""#).is_some_and(|fetched| fetched < checked) && checked < installed, "{s}");
        };
        for (repo, s) in repo_scripts() {
            check(&s, repo.fingerprint, &format!("-o /etc/apt/keyrings/{}.gpg", repo.name));
            assert!(!s.contains("gpg | gpg"), "{s}");
            assert!(s.contains(r#"--homedir "$d/gnupg""#) && !s.contains("--keyserver"), "{s}");
        }
        assert!(gcloud_script().contains(r#"repo="https://packages.cloud.google.com/apt"; "#));
    }

    #[test]
    fn the_aws_cli_bundle_must_be_signed_by_the_embedded_key() {
        let s = awscli_script();
        assert!(AWS_CLI_KEY.starts_with("-----BEGIN PGP PUBLIC KEY BLOCK-----") && AWS_CLI_KEY.trim_end().ends_with("-----END PGP PUBLIC KEY BLOCK-----"));
        assert!(s.contains(AWS_CLI_KEY.trim_end()), "the key is in the script");
        assert!(s.contains(r#"curl -fsSL "$url.sig" -o "$d/awscliv2.zip.sig""#), "{s}");
        // Verified before it is unpacked, let alone run.
        let verified = s.find(&format!("VALIDSIG {AWS_CLI_KEY_FINGERPRINT} ")).unwrap();
        assert!(verified < s.find("unzip -tq").unwrap() && verified < s.find(r#""$d/aws/install""#).unwrap(), "{s}");
        assert!(crate::platform::sh_parses(&s), "not valid sh: {s}");
    }

    #[test]
    fn no_vendor_install_script_runs_as_root() {
        // Regression: Docker (on distros without its apt repository), Azure CLI and gcloud ran
        // the vendor's install script (get.docker.com, aka.ms/InstallAzureCLIDeb,
        // sdk.cloud.google.com), which fetches its own key: no fingerprint held it.
        for s in root_scripts() {
            for script in ["get.docker.com", "aka.ms/InstallAzureCLIDeb", "sdk.cloud.google.com", "install.sh"] {
                assert!(!s.contains(script), "{script} in {s}");
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn docker_without_its_apt_repository_stops_and_says_where_to_get_it() {
        // Only the branch past the distro check: `dist` empty, nothing else run. Regression: the
        // slice began at the distro detection's own `[ -n "$dist" ]`, a syntax error that never ran
        // the refusal; bash echoes the failing line (URL included), so it passed on macOS only.
        let s = docker_script(None);
        let refuse = &s[s.find("[ -n \"$dist\" ] ||").unwrap()..s.find("; if [ -n \"$own\" ]").unwrap()];
        let out = std::process::Command::new("sh").arg("-c").arg(format!("dist=''; {refuse}; echo installed")).output().unwrap();
        let err = String::from_utf8_lossy(&out.stderr);
        assert_eq!(out.status.code(), Some(1), "the refusal's own exit, not a syntax error: {err}");
        assert!(!String::from_utf8_lossy(&out.stdout).contains("installed"));
        assert!(err.starts_with("Cyber CTF installs Docker on Debian") && err.contains("https://docs.docker.com/engine/install/"), "{err}");
    }

    #[cfg(unix)]
    #[test]
    fn a_key_with_another_fingerprint_stops_the_install() {
        if !have("gpg") {
            eprintln!("no gpg here: the key check isn't run");
            return;
        }
        let pinned = format!(r#"mkdir -p -m 700 "$d/gnupg"; {}; echo checked >&2"#, check_keys(&[AWS_CLI_KEY_FINGERPRINT], "AWS"));
        let (ok, err) = run_in_workdir(&pinned, &[("key", AWS_CLI_KEY.as_bytes())]);
        assert!(ok && err.contains("checked"), "{err}");
        let other = format!(r#"mkdir -p -m 700 "$d/gnupg"; {}; echo checked >&2"#, check_keys(&[DOCKER.fingerprint], "Docker"));
        let (ok, err) = run_in_workdir(&other, &[("key", AWS_CLI_KEY.as_bytes())]);
        assert!(!ok && !err.contains("checked"), "{err}");
        assert!(err.contains("Docker's signing key isn't the expected one (fingerprint 9DC858229FC7DD38854AE2D88D81803C0EBFCD88, got: FB5DB77FD5C118B80511ADA8A6310ACC4672475C)"), "{err}");
        let (ok, err) = run_in_workdir(&other, &[("key", b"<html>captive portal</html>")]);
        assert!(!ok && err.contains("got: )"), "{err}");
    }

    #[cfg(unix)]
    #[test]
    fn an_aws_cli_bundle_with_a_bad_signature_is_not_installed() {
        if !have("gpg") || !have("gpgv") {
            eprintln!("no gpg here: the signature check isn't run");
            return;
        }
        let verify = format!("{}; echo verified >&2", aws_verify());
        let (ok, err) = run_in_workdir(&verify, &[("awscliv2.zip", b"PK not really"), ("awscliv2.zip.sig", b"not a signature")]);
        assert!(!ok && !err.contains("verified"), "{err}");
        assert!(err.contains("The AWS CLI download isn't signed by AWS's key (FB5DB77FD5C118B80511ADA8A6310ACC4672475C)"), "{err}");
    }

    #[cfg(unix)]
    #[test]
    fn docker_picks_its_apt_repository_by_distro_or_the_distributions_own_docker() {
        let dir = std::env::temp_dir().join(format!("cyberctf-os-release-{}-{}", std::process::id(), rand::random::<u32>()));
        std::fs::create_dir_all(&dir).unwrap();
        let release = dir.join("os-release");
        assert!(DOCKER_DIST.contains(". /etc/os-release;") && DOCKER_DIST.contains("command -v apt-get >/dev/null"));
        let probe = DOCKER_DIST.replace("/etc/os-release", &release.display().to_string()).replace("command -v apt-get >/dev/null", "true");
        // Docker's repository as it is: the releases it serves, per distribution.
        let serves = r#"release_served() { case "$1/$2" in */debian/bookworm|*/debian/trixie|*/ubuntu/jammy|*/ubuntu/noble|*/raspbian/bookworm) return 0;; esac; return 1; }"#;
        let pick = |os: &str| {
            std::fs::write(&release, os).unwrap();
            let out = std::process::Command::new("sh").arg("-c").arg(format!("set -e; {serves}; {probe}; echo \"$dist/$codename/$own\"")).output().unwrap();
            assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
            String::from_utf8(out.stdout).unwrap().trim().to_string()
        };
        assert_eq!(pick("ID=ubuntu\nID_LIKE=debian\nVERSION_CODENAME=noble\nUBUNTU_CODENAME=noble\n"), "ubuntu/noble/");
        assert_eq!(pick("ID=linuxmint\nID_LIKE=\"ubuntu debian\"\nVERSION_CODENAME=wilma\nUBUNTU_CODENAME=noble\n"), "ubuntu/noble/");
        assert_eq!(pick("ID=debian\nVERSION_CODENAME=trixie\n"), "debian/trixie/");
        assert_eq!(pick("ID=raspbian\nID_LIKE=debian\nVERSION_CODENAME=bookworm\n"), "raspbian/bookworm/");
        assert_eq!(pick("ID=raspbian\nID_LIKE=debian\nVERSION_CODENAME=trixie\n"), "debian/trixie/");
        // A Debian derivative on the Debian release it names (LMDE): Docker's Debian repository.
        assert_eq!(pick("ID=linuxmint\nID_LIKE=debian\nVERSION_CODENAME=faye\nDEBIAN_CODENAME=bookworm\n"), "debian/bookworm/");
        // Regression: Kali (and Devuan) had no repository at all and the install stopped. Docker
        // serves neither release: the distribution's own Docker.
        assert_eq!(pick("ID=kali\nID_LIKE=debian\nVERSION_CODENAME=kali-rolling\nVERSION_ID=\"2025.4\"\n"), "debian/kali-rolling/1");
        assert_eq!(pick("ID=devuan\nID_LIKE=debian\nVERSION_CODENAME=excalibur\n"), "debian/excalibur/1");
        // A release newer than Docker serves yet: the distribution's own too.
        assert_eq!(pick("ID=debian\nVERSION_CODENAME=forky\n"), "debian/forky/1");
        assert_eq!(pick("ID=ubuntu\nID_LIKE=debian\nVERSION_CODENAME=resolute\nUBUNTU_CODENAME=resolute\n"), "ubuntu/resolute/1");
        // Neither Debian nor Ubuntu: nothing from here.
        assert_eq!(pick("ID=fedora\nVERSION_ID=43\n"), "//");
        assert_eq!(pick("ID=opensuse-tumbleweed\nID_LIKE=\"opensuse suse\"\n"), "//");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn where_dockers_repository_has_no_release_the_distributions_own_docker_and_compose_go_in() {
        let s = docker_script(Some("ada"));
        let own = &s[s.find("if [ -n \"$own\" ]; then").unwrap()..s.find("; else repo=").unwrap()];
        assert!(
            own.contains("apt-get install -y docker.io") && own.contains("docker-compose-v2") && own.contains("apt-get install -y docker-compose"),
            "{own}"
        );
        assert!(!own.contains("docker-ce") && !own.contains("download.docker.com/linux/$dist"), "{own}");
        // Either way the player joins the docker group, and the script parses.
        assert!(s.ends_with("echo 'Added ada to the docker group: log out and back in (or restart) for it to apply.'"), "{s}");
        assert!(crate::platform::sh_parses(&s), "not valid sh: {s}");
    }
}
