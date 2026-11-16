//! The shell scripts behind some installs: macOS casks opened in their own installer, and the
//! vendors' apt repositories on Linux. Every repository's signing key is checked against its
//! fingerprint, pinned here, before apt trusts it; no vendor install script runs as root.

use super::steps::Step;
use crate::platform::root_script::{NEED_CURL, NEED_GPG, NEED_UNZIP, RootScript, check_keys};

// Download the installer with brew (no admin needed), then open it so the tool's own
// native installer runs and the user finishes there (trusted OS prompts, no custom dialog).
// A disk image is mounted quietly and its .pkg opened in Installer, which comes to the front;
// only an app to drag into Applications (Docker Desktop, UTM) is shown in Finder, brought
// forward. Either way the log says where to finish: a window behind the app went unnoticed.
// `brew fetch` doesn't update Homebrew, and an old Homebrew can crash reading today's cask
// data (`undefined method 'first' for nil` in api/cask.rb), so a failed fetch updates
// Homebrew and tries once more.
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
    Step { program: "sh".into(), args: vec!["-c".into(), script], note: Some(format!("Downloading {name}…")), private: None }
}

/// The distro's codename as the vendors' repos name it (`$codename`): an Ubuntu derivative like
/// Mint uses its Ubuntu base's. For the Debian family (Debian itself, Kali, Devuan: `ID_LIKE`
/// without ubuntu), `$debian_base` names the suites to try when a vendor has none for this
/// codename: the base's (`DEBIAN_CODENAME`, LMDE), then Debian's stable releases, whose packages
/// a derivative runs (Kali's `kali-rolling` is served by no vendor; HashiCorp serves trixie and
/// bookworm). Empty elsewhere: an Ubuntu release Debian's packages don't fit.
const CODENAME: &str = r#"codename="$(. /etc/os-release && echo "${UBUNTU_CODENAME:-$VERSION_CODENAME}")"; [ -n "$codename" ] || codename="$(lsb_release -cs)"; debian_base="$(. /etc/os-release && case " $ID ${ID_LIKE:-} " in *ubuntu*) ;; *debian*) echo "${DEBIAN_CODENAME:-} trixie bookworm" ;; esac)""#;

/// A vendor's apt repository: where its packages and its signing key are, the key's pinned
/// fingerprint, and the name its keyring and its source list take here.
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
const RELEASE_SERVED: &str = r#"release_served() { served=$(curl -s -o /dev/null -w '%{http_code}' "$1/dists/$2/Release" || true); case "$served" in 2??) return 0 ;; 404) return 1 ;; *) echo "Couldn't reach $1 (HTTP ${served:-none}). Check this machine's internet access or proxy, then try again." >&2; exit 1 ;; esac; }"#;

/// curl, then [`RELEASE_SERVED`]: what a script needs before it asks a vendor's repository
/// anything.
fn fetch_prelude() -> String {
    format!("{NEED_CURL}; {RELEASE_SERVED}")
}

/// [`apt_repo_setup`] after [`fetch_prelude`]: a vendor's apt repository on a machine that may
/// lack curl.
fn apt_repo_script(r: &VendorRepo) -> String {
    format!("{}; {}", fetch_prelude(), apt_repo_setup(r))
}

/// Sets up `r` for apt, the same way for every vendor: the suite checked when releases have their
/// own ([`RELEASE_SERVED`]), the key downloaded, checked against its fingerprint and only then
/// made a keyring under `/etc/apt/keyrings`, the source restricted to this machine's architecture
/// and to that key, and apt's lists updated. `--batch --yes`: a second run (after a partial one)
/// overwrites the keyring instead of asking on a terminal it doesn't have. Runs in
/// a [`RootScript`], after [`fetch_prelude`].
fn apt_repo_setup(r: &VendorRepo) -> String {
    let VendorRepo { vendor, fingerprint, key_url, repo, suite, component, name, by_hand } = r;
    // No packages for this release: the Debian family's fallbacks (`$debian_base`, [`CODENAME`])
    // in order, the first the vendor serves taken as the suite; none served, the refusal.
    let suite_check = by_hand.map(|url| {
        format!(
            r#"; release_served "$repo" "{suite}" || {{ for alt in ${{debian_base:-}}; do if release_served "$repo" "$alt"; then echo "{vendor} has no packages for {suite}: installing its $alt ones, which this Debian derivative runs."; codename="$alt"; break; fi; done; release_served "$repo" "{suite}" || {{ echo "{vendor} has no packages for {suite} yet. Install it from {url}" >&2; exit 1; }}; }}"#
        )
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
pub fn terraform_script() -> RootScript {
    RootScript::new(format!("{CODENAME}; {}; apt-get install -y terraform", apt_repo_script(&HASHICORP)))
}

/// HashiCorp's apt repo, then Vagrant: the distributions' own package is older than labs need
/// (Debian 13 ships 2.3.8; labs need 2.4.0 or later), so a setup that installed it left a machine
/// that refused every VM lab.
pub fn vagrant_script() -> RootScript {
    const VAGRANT: VendorRepo = VendorRepo { by_hand: Some("https://developer.hashicorp.com/vagrant/install"), ..HASHICORP };
    RootScript::new(format!("{CODENAME}; {}; apt-get install -y vagrant", apt_repo_script(&VAGRANT)))
}

/// HashiCorp's apt repo, then the Vagrant VMware Utility, whose package generates its
/// certificates and enables and starts its service. `--reinstall`: an install that is there
/// already has its package configured again, which restarts a service that had stopped.
pub fn vmware_utility_script() -> RootScript {
    // HashiCorp's repository, but a release it doesn't serve yet is sent to the utility's own page.
    let repo = VendorRepo { by_hand: Some("https://developer.hashicorp.com/vagrant/install/vmware"), ..HASHICORP };
    RootScript::new(format!("{CODENAME}; {}; apt-get install -y --reinstall vagrant-vmware-utility", apt_repo_script(&repo)))
}

/// Oracle's VirtualBox archive signing key (`info@virtualbox.org`, "Oracle Corporation (VirtualBox
/// archive signing key)"), the one www.virtualbox.org serves as `oracle_vbox_2016.asc` and
/// download.virtualbox.org signs its `InRelease` files with.
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

/// What builds a kernel driver here, as `$build` (packages for apt): the compiler, make, perl and
/// kmod (`modprobe`), the running kernel's own headers (`linux-headers-$(uname -r)`), and its
/// flavour's header package, which brings each later kernel's headers with it
/// (`linux-headers-amd64` on Debian and Kali, `linux-headers-generic` on Ubuntu: the release's
/// tail that has a header package). When the package sources no longer have the running kernel's
/// headers (a machine not restarted since a kernel update: Debian's archive drops older kernels'),
/// the flavour's kernel comes with its headers and `$reboot` is set: the driver is built when the
/// computer starts that kernel (VirtualBox's `vboxdrv.sh start`, DKMS at the kernel's install).
/// Requesting the flavour's headers alone, as VirtualBox's recommends did, brought that newer
/// kernel's headers but not the running one's, so the driver never built. The other families
/// name them otherwise, where the guided setup doesn't install yet (it installs through apt):
/// Fedora `kernel-devel-$(uname -r)`, openSUSE `kernel-default-devel`, Arch `linux-headers`.
const KERNEL_BUILD: &str = r#"kver="$(uname -r)"; flavour=""; f="$kver"; while [ "${f#*-}" != "$f" ]; do f="${f#*-}"; if apt-cache policy "linux-headers-$f" 2>/dev/null | grep -q 'Candidate: [0-9]'; then flavour="$f"; break; fi; done; build="gcc make perl kmod"; reboot=""; if apt-cache policy "linux-headers-$kver" 2>/dev/null | grep -q 'Candidate: [0-9]'; then build="$build linux-headers-$kver${flavour:+ linux-headers-$flavour}"; elif [ -n "$flavour" ]; then build="$build linux-image-$flavour linux-headers-$flavour"; reboot=1; else echo "VirtualBox builds its kernel driver with the running kernel's headers (linux-headers-$kver), which this machine's package sources don't have. Update the kernel, restart the computer, then install VirtualBox again." >&2; exit 1; fi"#;

/// The distro's own `virtualbox` when it has one (Ubuntu), else Oracle's apt repo and its newest
/// `virtualbox-X.Y` (Debian ships none in main, so `apt-get install virtualbox` failed); with what
/// builds its kernel driver ([`KERNEL_BUILD`]), and without its recommends: a PDF viewer, Qt's
/// translations and the newest kernel's headers (and so that kernel) came with them, which a
/// headless VirtualBox that Vagrant drives never uses. When the running kernel's headers were gone,
/// it says the driver waits for a restart.
pub fn virtualbox_script() -> RootScript {
    RootScript::new(format!(
        r#"apt-get update; {KERNEL_BUILD}; if apt-cache policy virtualbox 2>/dev/null | grep -q 'Candidate: [0-9]'; then apt-get install -y --no-install-recommends virtualbox $build; else {CODENAME}; {}; pkg="$(apt-cache search --names-only '^virtualbox-[0-9]+\.[0-9]+$' | awk '{{print $1}}' | sort -V | tail -n 1)"; [ -n "$pkg" ] || {{ echo "No VirtualBox package found in Oracle's repo. Install it from https://www.virtualbox.org/wiki/Linux_Downloads" >&2; exit 1; }}; apt-get install -y --no-install-recommends "$pkg" $build; fi; [ -z "$reboot" ] || echo "VirtualBox is installed. This computer runs a kernel ($kver) whose headers its package sources no longer have, so the newer kernel came with its headers: restart the computer to start it, and VirtualBox's driver is built then.""#,
        apt_repo_script(&ORACLE)
    ))
}

/// Docker's release key (`docker@docker.com`, Docker Release (CE deb)), the one download.docker.com
/// serves at `/linux/{ubuntu,debian,raspbian}/gpg` and signs its `InRelease` files with (from its
/// signing subkey D3306A018370199E527AE7997EA0A9C3F273FCD8); keyserver.ubuntu.com has the same.
/// Its repository is one per distro (`$dist`, see [`DOCKER_DIST`]), whose release is checked there
/// (a release it doesn't serve gets the distribution's own Docker), so not again here.
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
pub fn docker_script(user: Option<&str>) -> RootScript {
    use std::fmt::Write as _;
    let mut s = format!(
        "{}; {DOCKER_DIST}; [ -n \"$dist\" ] || {{ echo 'Cyber CTF installs Docker on Debian, Ubuntu and the distributions built on them. Install Docker Engine from your distribution or https://docs.docker.com/engine/install/, then check again.' >&2; exit 1; }}; if [ -n \"$own\" ]; then {DISTRIBUTION_DOCKER}; else {}; apt-get install -y docker-ce docker-ce-cli containerd.io docker-buildx-plugin docker-compose-plugin; fi",
        fetch_prelude(),
        apt_repo_setup(&DOCKER)
    );
    if let Some(u) = user.filter(|u| valid_user(u)) {
        let _ = write!(s, "; usermod -aG docker {u}; echo 'Added {u} to the docker group: log out and back in (or restart) for it to apply.'");
    }
    RootScript::new(s)
}

/// Which of Docker's apt repositories fits this system (`$dist`, empty when none does) and its
/// codename there; `$own` set when that repository has no packages for it ([`RELEASE_SERVED`]: a
/// 404, never an unreachable server, which stops the script), so the distribution's own Docker is
/// installed. A derivative is known by `ID_LIKE`, on its base's release: Ubuntu's
/// (`UBUNTU_CODENAME`, Mint), Debian's (`DEBIAN_CODENAME`, LMDE), else its own (Kali, Devuan),
/// which Docker doesn't serve. Raspbian Trixie has none of Docker's: Debian's serves it (as
/// get.docker.com does).
const DOCKER_DIST: &str = r#". /etc/os-release; dist=""; own=""; codename="${VERSION_CODENAME:-}"; case "$ID" in ubuntu|debian|raspbian) dist="$ID";; *) case " ${ID_LIKE:-} " in *" ubuntu "*) dist=ubuntu;; *" debian "*) dist=debian; codename="${DEBIAN_CODENAME:-$codename}";; esac;; esac; [ "$dist" != ubuntu ] || codename="${UBUNTU_CODENAME:-$codename}"; [ "$dist$codename" != raspbiantrixie ] || dist=debian; command -v apt-get >/dev/null || dist=""; if [ -n "$dist" ] && ! release_served "https://download.docker.com/linux/$dist" "$codename"; then own=1; fi"#;

/// The distribution's own Docker: `docker.io`, and its Compose plugin (`docker-compose-v2` on
/// Ubuntu, `docker-compose` on Debian and Kali), from the distribution's archive.
const DISTRIBUTION_DOCKER: &str = r#"echo "Docker's apt repository has no packages for $dist $codename: installing the distribution's own Docker (docker.io)."; apt-get update; apt-get install -y docker.io; if apt-cache show docker-compose-v2 >/dev/null 2>&1; then apt-get install -y docker-compose-v2; else apt-get install -y docker-compose; fi"#;

/// The AWS CLI team's signing key (`aws-cli@amazon.com`, created 2019-09-18, expires 2027-07-01 as
/// published), as AWS publishes it in "Installing or updating to the latest version of the AWS
/// CLI" (Linux, "Verify the integrity and authenticity of the downloaded zip file"). An expired
/// copy still verifies older signatures; refresh it from that page when AWS extends it.
const AWS_CLI_KEY: &str = include_str!("keys/aws-cli.asc");
/// Its fingerprint, as that page gives it (FB5D B77F D5C1 18B8 0511 ADA8 A631 0ACC 4672 475C).
pub const AWS_CLI_KEY_FINGERPRINT: &str = "FB5DB77FD5C118B80511ADA8A6310ACC4672475C";

/// AWS CLI v2 from Amazon's official bundle, unpacked and installed from a private directory.
/// The distro `awscli` package is v1, which has no `aws login` (the app needs v2 >= 2.32).
/// `uname -m` is `x86_64` or `aarch64`, matching the URLs. The bundle must carry a valid signature
/// (its `.sig`) by AWS's key, embedded here ([`AWS_CLI_KEY`]): gpgv, with only that key, must
/// report it by its fingerprint. Then `unzip -t` checks the archive is whole before anything
/// in it runs.
pub fn awscli_script() -> RootScript {
    RootScript::new(format!(
        r#"{NEED_CURL}; url="https://awscli.amazonaws.com/awscli-exe-linux-$(uname -m).zip"; curl -fsSL "$url" -o "$d/awscliv2.zip"; curl -fsSL "$url.sig" -o "$d/awscliv2.zip.sig"; {}; {NEED_UNZIP}; unzip -tq "$d/awscliv2.zip" >/dev/null; unzip -oq "$d/awscliv2.zip" -d "$d"; "$d/aws/install" --update"#,
        aws_verify()
    ))
}

/// Stops the script unless `"$d/awscliv2.zip.sig"` is a valid signature of `"$d/awscliv2.zip"`
/// by [`AWS_CLI_KEY`].
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
pub fn azurecli_script() -> RootScript {
    RootScript::new(format!("{CODENAME}; {}; apt-get install -y azure-cli", apt_repo_script(&MICROSOFT)))
}

/// Google Cloud's package signing key ("Artifact Registry Repository Signer"), the one
/// packages.cloud.google.com serves at `/apt/doc/apt-key.gpg` and signs the cloud-sdk
/// repository's `InRelease` files with. One suite for every release.
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
pub fn gcloud_script() -> RootScript {
    RootScript::new(format!("{}; apt-get install -y google-cloud-cli", apt_repo_script(&GOOGLE)))
}

/// libvirt, with what `vagrant plugin install vagrant-libvirt` compiles against: libvirt's headers
/// (`libvirt-dev`), a C compiler and make (`build-essential`), `pkg-config`, and Ruby's headers
/// for a distribution's Vagrant (which builds on the system Ruby). Without them the plugin's
/// install fails building its native extension. Then the player joins the `libvirt` group (and
/// `kvm`, where the distribution has one), so Vagrant reaches `qemu:///system` and `/dev/kvm`
/// without root; the groups apply from the next login.
pub fn libvirt_script(user: Option<&str>) -> RootScript {
    // virtiofsd: libvirt's shared folders (the attack box's /shared) need it, and it is its own
    // package from Debian 12 and Ubuntu 23.10 on (QEMU's before); a release without it skips it.
    // Separate statements: a failure inside an `&&` chain doesn't stop a script that runs `set -e`.
    then_libvirt_groups(
        "apt-get update; apt-get install -y libvirt-daemon-system libvirt-dev build-essential pkg-config ruby-dev virt-manager; \
         if apt-cache show virtiofsd >/dev/null 2>&1; then apt-get install -y virtiofsd; fi",
        user,
    )
}

/// QEMU and its disk tools, then the player into the group `/dev/kvm` belongs to, so vagrant-qemu
/// boots VMs with KVM (`accel=kvm`, as every lab's Vagrantfile asks on Linux) without root; the
/// group applies from the next login. With `genisoimage`, which has `mkisofs`: vagrant-qemu gives
/// a VM on a lab network its settings in a cloud-init seed, an ISO Vagrant builds with `mkisofs`
/// on Linux ([`crate::runtime::providers::needs_mkisofs`]). Every Debian and Ubuntu release has
/// the package; Debian's `xorriso` has no `mkisofs`, only `xorrisofs`. And `virtiofsd`, which
/// vagrant-qemu starts to share a folder of this computer with a VM (the attack box's shared
/// folder): its own package from Debian 12 and Ubuntu 23.10 on, QEMU's own
/// (`/usr/lib/qemu/virtiofsd`) before, so a release without the package skips it.
pub fn qemu_script(user: Option<&str>) -> RootScript {
    // Separate statements: a failure inside an `&&` chain doesn't stop a script that runs `set -e`.
    then_kvm_group(
        "apt-get update; apt-get install -y qemu-system qemu-utils genisoimage; \
         if apt-cache show virtiofsd >/dev/null 2>&1; then apt-get install -y virtiofsd; fi",
        user,
    )
}

/// The KVM device.
const KVM_DEVICE: &str = "/dev/kvm";

/// `install`, then the player `user` (a valid login name only) into the group the KVM device
/// belongs to: it applies from the next login.
fn then_kvm_group(install: &str, user: Option<&str>) -> RootScript {
    match user.filter(|u| valid_user(u)) {
        Some(u) => RootScript::new(format!("{install}; {}", join_kvm_group(u, KVM_DEVICE))),
        None => RootScript::new(install),
    }
}

/// `install`, then the player `user` (a valid login name only) into the `libvirt` group and the
/// KVM device's: they apply from the next login.
fn then_libvirt_groups(install: &str, user: Option<&str>) -> RootScript {
    match user.filter(|u| valid_user(u)) {
        Some(u) => RootScript::new(format!(
            "{install}; usermod -aG libvirt {u}; {}; echo 'Added {u} to the libvirt group: log out and back in (or restart) for it to apply.'",
            join_kvm_group(u, KVM_DEVICE)
        )),
        None => RootScript::new(install),
    }
}

/// Player `u` into the group `dev` (the KVM device) belongs to, saying so: `kvm` where a
/// distribution makes it `0660` (Debian, Ubuntu, Kali, openSUSE); joining it does no harm where
/// it is `0666` (Fedora, Arch). Without the device (virtualization off in the firmware), or when it
/// is root's, the `kvm` group, where the distribution has one.
fn join_kvm_group(u: &str, dev: &str) -> String {
    format!(
        "g=$(stat -c %G {dev} 2>/dev/null || true); case \"$g\" in ''|root|UNKNOWN) g=kvm;; esac; if getent group \"$g\" >/dev/null; then usermod -aG \"$g\" {u} && echo \"Added {u} to the $g group, which opens {dev}: log out and back in (or restart) for it to apply.\"; fi"
    )
}

/// A login name safe to put in a shell script unquoted.
fn valid_user(u: &str) -> bool {
    !u.is_empty()
        && u.len() <= 32
        && u.chars().next().is_some_and(|c| c.is_ascii_lowercase() || c == '_')
        && u.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.')
}

// --- dnf: Fedora and the distributions built on it --------------------------------------------

/// A vendor's rpm repository, set up the same way for every vendor ([`rpm_repo_setup`]): its
/// signing key downloaded and checked against the pinned fingerprint, kept under
/// `/etc/pki/rpm-gpg` and imported, then a repo file under `/etc/yum.repos.d` that trusts that
/// key alone (`gpgcheck=1`). `RELEASE` in the base URL is the Fedora release the repository is
/// read for: this one, else the newest earlier one it serves (HashiCorp serves a new Fedora
/// release weeks after it ships), else the refusal, as the apt repositories' `release_served`.
struct RpmRepo {
    vendor: &'static str,
    fingerprint: &'static str,
    key_url: &'static str,
    /// The base URL with `RELEASE` for the Fedora release and `$basearch` for dnf's architecture.
    baseurl: &'static str,
    /// The key file's and the repo file's name.
    name: &'static str,
    /// Where to install from instead when the repository serves no release near this one.
    by_hand: &'static str,
}

/// `rpm_served <base url>`: whether an rpm repository at that URL answers (its `repodata/repomd.xml`):
/// a 2xx is yes and a 404 no; anything else stops the script (an unreachable server is never "no
/// packages"), as [`RELEASE_SERVED`] does for apt.
const RPM_SERVED: &str = r#"rpm_served() { served=$(curl -s -o /dev/null -w '%{http_code}' "$1/repodata/repomd.xml" || true); case "$served" in 2??) return 0 ;; 404) return 1 ;; *) echo "Couldn't reach $1 (HTTP $served). Check this machine's internet access or proxy, then try again." >&2; exit 1 ;; esac; }"#;

/// The script that sets `r` up for dnf, after [`fetch_prelude`] in a [`RootScript`]: the
/// release the repository serves found (`$rel`, this Fedora's or the newest earlier one of the
/// three), the key checked and imported, the repo file written.
fn rpm_repo_setup(r: &RpmRepo) -> String {
    let RpmRepo { vendor, fingerprint, key_url, baseurl, name, by_hand } = r;
    let probe = baseurl.replace("$basearch", "$(uname -m)");
    format!(
        r#"{RPM_SERVED}; this=$(rpm -E %fedora); rel=""; for r in "$this" $((this - 1)) $((this - 2)); do if rpm_served "{probe_r}"; then rel=$r; break; fi; done; [ -n "$rel" ] || {{ echo "{vendor} has no packages for Fedora $this yet. Install it from {by_hand}" >&2; exit 1; }}; [ "$rel" = "$this" ] || echo "{vendor} has no packages for Fedora $this: installing its Fedora $rel ones."; {NEED_GPG}; curl -fsSL "{key_url}" -o "$d/key"; {check}; install -m 0644 "$d/key" /etc/pki/rpm-gpg/RPM-GPG-KEY-{name}; rpm --import /etc/pki/rpm-gpg/RPM-GPG-KEY-{name}; printf '[{name}]\nname={vendor}\nbaseurl=%s\nenabled=1\ngpgcheck=1\ngpgkey=file:///etc/pki/rpm-gpg/RPM-GPG-KEY-{name}\n' "{base_rel}" >/etc/yum.repos.d/{name}.repo"#,
        probe_r = probe.replace("RELEASE", "$r"),
        // The release found expands; dnf's own `$basearch` stays for dnf.
        base_rel = baseurl.replace("$basearch", "\\$basearch").replace("RELEASE", "$rel"),
        check = check_keys(&[fingerprint], vendor),
    )
}

/// Docker's rpm repository for Fedora, its key pinned (`docker@docker.com`, the key
/// download.docker.com/linux/fedora/gpg serves; not the apt one).
const DOCKER_RPM: RpmRepo = RpmRepo {
    vendor: "Docker",
    fingerprint: "060A61C51B558A7F742B77AAC52FEB6B621E9F35",
    key_url: "https://download.docker.com/linux/fedora/gpg",
    baseurl: "https://download.docker.com/linux/fedora/RELEASE/$basearch/stable",
    name: "docker-ce",
    by_hand: "https://docs.docker.com/engine/install/fedora/",
};

/// HashiCorp's rpm repository (Vagrant, Terraform), signed by the same key as its apt one.
const HASHICORP_RPM: RpmRepo = RpmRepo {
    vendor: "HashiCorp",
    fingerprint: "D55C0D1AC78A8D8126CB631CFC9CA96ACA026560",
    key_url: "https://rpm.releases.hashicorp.com/gpg",
    baseurl: "https://rpm.releases.hashicorp.com/fedora/RELEASE/$basearch/stable",
    name: "hashicorp",
    by_hand: "https://developer.hashicorp.com/vagrant/install",
};

/// Docker Engine on Fedora from Docker's own repository, its key pinned, then its service on;
/// the player joins the `docker` group as on Debian.
pub fn docker_dnf_script(user: Option<&str>) -> RootScript {
    use std::fmt::Write as _;
    let mut s = format!(
        "{}; {}; dnf install -y docker-ce docker-ce-cli containerd.io docker-buildx-plugin docker-compose-plugin; systemctl enable --now docker",
        fetch_prelude(),
        rpm_repo_setup(&DOCKER_RPM)
    );
    if let Some(u) = user.filter(|u| valid_user(u)) {
        let _ = write!(s, "; usermod -aG docker {u}; echo 'Added {u} to the docker group: log out and back in (or restart) for it to apply.'");
    }
    RootScript::new(s)
}

/// Vagrant on Fedora from HashiCorp's rpm repository (the distribution has none).
pub fn vagrant_dnf_script() -> RootScript {
    RootScript::new(format!("{}; {}; dnf install -y vagrant", fetch_prelude(), rpm_repo_setup(&HASHICORP_RPM)))
}

/// Terraform on Fedora from the same repository.
pub fn terraform_dnf_script() -> RootScript {
    RootScript::new(format!("{}; {}; dnf install -y terraform", fetch_prelude(), rpm_repo_setup(&HASHICORP_RPM)))
}

/// VirtualBox on Fedora from RPM Fusion (Oracle's own repository lags the Fedora releases by
/// months), its kernel module built here for the running kernel (akmods),
/// then the player into `vboxusers`. RPM Fusion's release package carries its signing key, which
/// dnf imports as it installs it.
pub fn virtualbox_dnf_script(user: Option<&str>) -> RootScript {
    use std::fmt::Write as _;
    let mut s = r#"dnf install -y "https://mirrors.rpmfusion.org/free/fedora/rpmfusion-free-release-$(rpm -E %fedora).noarch.rpm"; dnf install -y VirtualBox akmod-VirtualBox "kernel-devel-$(uname -r)" kernel-headers; akmods --force --kernels "$(uname -r)"; modprobe vboxdrv || echo "VirtualBox's driver isn't loaded yet: restart the computer to start it, and VirtualBox's driver is built then.""#.to_string();
    if let Some(u) = user.filter(|u| valid_user(u)) {
        let _ = write!(s, "; usermod -aG vboxusers {u}");
    }
    RootScript::new(s)
}

/// libvirt on Fedora with what vagrant-libvirt builds against (libvirt's headers, a C compiler,
/// pkg-config, Ruby's headers) and virtiofsd for the shared folders, its service on, then the
/// player into the `libvirt` group (and the KVM device's).
pub fn libvirt_dnf_script(user: Option<&str>) -> RootScript {
    then_libvirt_groups(
        "dnf install -y libvirt libvirt-daemon-kvm qemu-kvm virt-install virt-manager libvirt-devel gcc make pkgconf-pkg-config ruby-devel virtiofsd; systemctl enable --now libvirtd",
        user,
    )
}

/// QEMU and its disk tools on Fedora, with `genisoimage` for vagrant-qemu's cloud-init seeds and
/// `virtiofsd` for its shared folders (its own package from Fedora 36 on), then the player into
/// the KVM device's group.
pub fn qemu_dnf_script(user: Option<&str>) -> RootScript {
    then_kvm_group("dnf install -y qemu-kvm qemu-img genisoimage virtiofsd", user)
}

#[cfg(test)]
mod dnf_tests {
    use super::*;

    #[test]
    fn every_dnf_script_is_valid_sh_and_installs_through_dnf_alone() {
        for (name, s) in [
            ("docker", docker_dnf_script(Some("alice"))),
            ("vagrant", vagrant_dnf_script()),
            ("terraform", terraform_dnf_script()),
            ("virtualbox", virtualbox_dnf_script(Some("alice"))),
            ("libvirt", libvirt_dnf_script(Some("alice"))),
            ("qemu", qemu_dnf_script(Some("alice"))),
        ] {
            assert!(crate::platform::sh_parses(&s), "{name}: not valid sh: {s}");
            // The preludes install curl and gpg through whichever manager is there; the rest is dnf's.
            let own = s.replace(NEED_CURL, "").replace(NEED_GPG, "").replace(NEED_UNZIP, "");
            assert!(!own.contains("apt-get"), "{name} installs through apt: {s}");
            assert!(s.contains("dnf install -y"), "{name}: {s}");
        }
    }

    #[test]
    fn a_vendors_rpm_repository_is_pinned_by_key_and_read_for_a_release_it_serves() {
        let s = rpm_repo_setup(&HASHICORP_RPM);
        assert!(s.contains(r#"[ "$k" = "D55C0D1AC78A8D8126CB631CFC9CA96ACA026560" ]"#), "the key pinned: {s}");
        assert!(
            s.contains("rpm --import /etc/pki/rpm-gpg/RPM-GPG-KEY-hashicorp")
                && s.contains("gpgcheck=1")
                && s.contains("gpgkey=file:///etc/pki/rpm-gpg/RPM-GPG-KEY-hashicorp"),
            "{s}"
        );
        assert!(
            s.contains(r#"for r in "$this" $((this - 1)) $((this - 2))"#)
                && s.contains(r#"rpm_served "https://rpm.releases.hashicorp.com/fedora/$r/$(uname -m)/stable""#),
            "this release, else the two before: {s}"
        );
        assert!(s.contains(r#""https://rpm.releases.hashicorp.com/fedora/$rel/\$basearch/stable""#), "the release found, dnf's own $basearch: {s}");
        assert!(s.contains("HashiCorp has no packages for Fedora $this yet. Install it from https://developer.hashicorp.com/vagrant/install"), "{s}");
        let docker = rpm_repo_setup(&DOCKER_RPM);
        assert!(docker.contains("060A61C51B558A7F742B77AAC52FEB6B621E9F35") && docker.contains("/etc/yum.repos.d/docker-ce.repo"), "{docker}");
    }

    #[cfg(unix)]
    #[test]
    fn a_release_the_repository_doesnt_serve_falls_back_to_the_newest_earlier_one() {
        // HashiCorp serving Fedora 44 and not yet 45.
        let s = rpm_repo_setup(&HASHICORP_RPM);
        let head = &s[..s.find("; command -v gpg").unwrap()];
        let bin = std::env::temp_dir().join(format!("cyberctf-rpm-probe-{}-{}", std::process::id(), rand::random::<u32>()));
        std::fs::create_dir_all(&bin).unwrap();
        crate::testing::write_executable(bin.join("curl"), "#!/bin/sh\ncase \"$*\" in */fedora/44/*) printf 200 ;; *) printf 404 ;; esac\n");
        crate::testing::write_executable(bin.join("rpm"), "#!/bin/sh\necho 45\n");
        let out = std::process::Command::new("sh")
            .arg("-c")
            .arg(format!("set -e; {head}; echo \"rel=$rel\""))
            .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
            .output()
            .unwrap();
        std::fs::remove_dir_all(&bin).unwrap();
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(
            stdout.contains("HashiCorp has no packages for Fedora 45: installing its Fedora 44 ones.") && stdout.trim_end().ends_with("rel=44"),
            "{stdout}"
        );
    }

    #[test]
    fn virtualbox_on_fedora_comes_from_rpm_fusion_with_its_module_built_here() {
        let s = virtualbox_dnf_script(Some("alice"));
        assert!(
            s.contains("rpmfusion-free-release-$(rpm -E %fedora).noarch.rpm")
                && s.contains("akmod-VirtualBox")
                && s.contains("akmods --force")
                && s.contains("usermod -aG vboxusers alice"),
            "{s}"
        );
    }
}
#[cfg(test)]
mod tests;
