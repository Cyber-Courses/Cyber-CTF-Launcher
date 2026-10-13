//! The shell scripts behind some installs: macOS casks opened in their own installer, and the
//! vendors' apt repositories on Linux.

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

/// The start of every script run as root that downloads something (see [`PRIVATE_WORKDIR`]), as a
/// literal for the scripts built with `concat!`.
#[cfg(any(target_os = "linux", test))]
macro_rules! private_workdir {
    () => {
        r#"set -e; d="$(mktemp -d)"; trap 'rm -rf "$d"' EXIT"#
    };
}

/// gpg, for the vendors' signing keys (Debian's minimal installs have only gpgv), and a private
/// keyring folder for it in the work directory, so root's own keyring is never read or changed.
#[cfg(any(target_os = "linux", test))]
macro_rules! need_gpg {
    () => {
        r#"command -v gpg >/dev/null || { apt-get update && apt-get install -y gnupg; }; mkdir -p -m 700 "$d/gnupg""#
    };
}

/// Stops the script unless `"$d/key"`, a vendor's signing key just downloaded, is exactly one key
/// with the fingerprint pinned here (`$fpr`, 40 hex digits). TLS alone trusted whatever the
/// vendor's server sent the first time; a swapped key would sign the packages apt then installs
/// as root.
#[cfg(any(target_os = "linux", test))]
macro_rules! check_key {
    ($fpr:expr, $vendor:literal) => {
        concat!(
            r#"k="$(gpg --homedir "$d/gnupg" --batch --show-keys --with-colons "$d/key" | awk -F: '$1=="pub"{n++} $1=="fpr"&&!f{f=$10} END{print n+0" "f}')""#,
            r#"; [ "$k" = "1 "#,
            $fpr,
            r#"" ] || { echo ""#,
            $vendor,
            r#"'s signing key isn't the expected one (fingerprint "#,
            $fpr,
            r#", got: $k). Nothing was installed." >&2; exit 1; }"#
        )
    };
}

/// HashiCorp's package signing key (`security+packaging@hashicorp.com`, created 2026-09-09,
/// expires 2031-09-08), the one apt.releases.hashicorp.com serves at `/gpg` and signs its
/// `InRelease` files with; keys.openpgp.org and keyserver.ubuntu.com have the same. When HashiCorp
/// rotates it, installs stop with "isn't the expected one" until this is updated.
#[cfg(any(target_os = "linux", test))]
macro_rules! hashicorp_key_fingerprint {
    () => {
        "D55C0D1AC78A8D8126CB631CFC9CA96ACA026560"
    };
}
#[cfg(test)]
const HASHICORP_KEY_FINGERPRINT: &str = hashicorp_key_fingerprint!();

/// HashiCorp's apt repo, then terraform. The codename is the distro's as the vendors' repos name
/// it (an Ubuntu derivative like Mint uses its Ubuntu base's). The repo key must be HashiCorp's
/// pinned one (`hashicorp_key_fingerprint!`). `--batch --yes`: a second run (after a partial
/// one) overwrites the keyring instead of asking on a terminal it doesn't have.
#[cfg(any(target_os = "linux", test))]
pub const TERRAFORM_SCRIPT: &str = concat!(
    private_workdir!(),
    "; ",
    need_gpg!(),
    "; ",
    r#"codename="$(. /etc/os-release && echo "${UBUNTU_CODENAME:-$VERSION_CODENAME}")"; [ -n "$codename" ] || codename="$(lsb_release -cs)""#,
    r#"; curl -fsSL https://apt.releases.hashicorp.com/gpg -o "$d/key"; "#,
    check_key!(hashicorp_key_fingerprint!(), "HashiCorp"),
    r#"; gpg --batch --yes --dearmor --homedir "$d/gnupg" -o /usr/share/keyrings/hashicorp-archive-keyring.gpg "$d/key""#,
    "; echo \"deb [signed-by=/usr/share/keyrings/hashicorp-archive-keyring.gpg] https://apt.releases.hashicorp.com $codename main\" > /etc/apt/sources.list.d/hashicorp.list",
    "; apt-get update && apt-get install -y terraform"
);

/// The distro's own `virtualbox` when it has one (Ubuntu), else Oracle's apt repo and its
/// newest `virtualbox-X.Y` (Debian ships none in main, so `apt-get install virtualbox` failed).
#[cfg(any(target_os = "linux", test))]
pub const VIRTUALBOX_SCRIPT: &str = concat!(
    "set -e; apt-get update; ",
    "if apt-cache policy virtualbox 2>/dev/null | grep -q 'Candidate: [0-9]'; then apt-get install -y virtualbox; exit 0; fi; ",
    r#"codename="$(. /etc/os-release && echo "${UBUNTU_CODENAME:-$VERSION_CODENAME}")"; [ -n "$codename" ] || codename="$(lsb_release -cs)""#,
    "; repo=https://download.virtualbox.org/virtualbox/debian",
    "; curl -fsI \"$repo/dists/$codename/Release\" >/dev/null || { echo \"Oracle has no VirtualBox packages for $codename yet. Install it from https://www.virtualbox.org/wiki/Linux_Downloads\" >&2; exit 1; }",
    "; curl -fsSL https://www.virtualbox.org/download/oracle_vbox_2016.asc | gpg --batch --yes --dearmor -o /usr/share/keyrings/oracle-virtualbox-2016.gpg",
    "; echo \"deb [arch=$(dpkg --print-architecture) signed-by=/usr/share/keyrings/oracle-virtualbox-2016.gpg] $repo $codename contrib\" > /etc/apt/sources.list.d/virtualbox.list",
    "; apt-get update",
    "; pkg=\"$(apt-cache search --names-only '^virtualbox-[0-9]+\\.[0-9]+$' | awk '{print $1}' | sort -V | tail -n 1)\"",
    "; [ -n \"$pkg\" ] || { echo \"No VirtualBox package found in Oracle's repo. Install it from https://www.virtualbox.org/wiki/Linux_Downloads\" >&2; exit 1; }",
    "; apt-get install -y \"$pkg\""
);

/// Docker's release key (`docker@docker.com`, Docker Release (CE deb)), the one download.docker.com
/// serves at `/linux/{ubuntu,debian,raspbian}/gpg` and signs its `InRelease` files with (from its
/// signing subkey D3306A018370199E527AE7997EA0A9C3F273FCD8); keyserver.ubuntu.com has the same.
#[cfg(any(target_os = "linux", test))]
macro_rules! docker_key_fingerprint {
    () => {
        "9DC858229FC7DD38854AE2D88D81803C0EBFCD88"
    };
}
#[cfg(test)]
const DOCKER_KEY_FINGERPRINT: &str = docker_key_fingerprint!();

/// Docker Engine, then the player into the `docker` group so the CLI can reach the daemon without
/// root (pkexec runs as root, so `$USER` there is root: the name comes from the app). The group
/// applies from the next login.
///
/// On Ubuntu, Debian and Raspbian (and Ubuntu's derivatives, on their Ubuntu base) it sets up
/// Docker's apt repository itself, the way Docker documents it, with Docker's key checked against
/// its pinned fingerprint (`docker_key_fingerprint!`). Docker's convenience script
/// (get.docker.com) can't be held to a key: it fetches the key and installs the packages signed
/// with it in one go. So it only runs, downloaded first, on the other distros, where it knows the
/// package manager.
#[cfg(any(target_os = "linux", test))]
pub fn docker_script(user: Option<&str>) -> String {
    let mut s = format!(
        "{PRIVATE_WORKDIR}; {DOCKER_DIST}; if [ -n \"$dist\" ]; then {DOCKER_APT_REPO}; else echo 'No Docker apt repository for this system: running Docker'\\''s install script.'; {}; fi",
        fetch_into_workdir("https://get.docker.com", "sh"),
    );
    if let Some(u) = user.filter(|u| valid_user(u)) {
        s.push_str(&format!("; usermod -aG docker {u}; echo 'Added {u} to the docker group: log out and back in (or restart) for it to apply.'"));
    }
    s
}

/// Which of Docker's apt repositories fits this system (`$dist`, empty when none does) and its
/// codename there. Raspbian Trixie has none: Debian's serves it (as get.docker.com does).
#[cfg(any(target_os = "linux", test))]
const DOCKER_DIST: &str = r#". /etc/os-release; dist=""; codename="${VERSION_CODENAME:-}"; case "$ID" in ubuntu|debian|raspbian) dist="$ID";; *) case " ${ID_LIKE:-} " in *" ubuntu "*) dist=ubuntu;; esac;; esac; [ "$dist" != ubuntu ] || codename="${UBUNTU_CODENAME:-$codename}"; [ "$dist$codename" != raspbiantrixie ] || dist=debian; command -v apt-get >/dev/null || dist="""#;

/// Docker's apt repository for `$dist` / `$codename`, its key checked, then Docker Engine. The
/// keyring is where get.docker.com puts it, so a system set up by either one is the same.
#[cfg(any(target_os = "linux", test))]
const DOCKER_APT_REPO: &str = concat!(
    r#"repo="https://download.docker.com/linux/$dist""#,
    r#"; curl -fsI "$repo/dists/$codename/Release" >/dev/null || { echo "Docker has no packages for $dist $codename yet. Install Docker from https://docs.docker.com/engine/install/" >&2; exit 1; }"#,
    "; ",
    need_gpg!(),
    r#"; curl -fsSL "$repo/gpg" -o "$d/key"; "#,
    check_key!(docker_key_fingerprint!(), "Docker"),
    r#"; install -m 0755 -d /etc/apt/keyrings; install -m 0644 "$d/key" /etc/apt/keyrings/docker.asc"#,
    r#"; echo "deb [arch=$(dpkg --print-architecture) signed-by=/etc/apt/keyrings/docker.asc] $repo $codename stable" > /etc/apt/sources.list.d/docker.list"#,
    "; apt-get update; apt-get install -y docker-ce docker-ce-cli containerd.io docker-buildx-plugin docker-compose-plugin"
);

/// The start of every script run as root that downloads something: stop at the first failure,
/// and work in a fresh private directory (`mktemp -d`, mode 0700, removed on exit). A fixed path
/// such as `/tmp/aws` could be prepared by any local user beforehand and then run as root.
#[cfg(any(target_os = "linux", test))]
const PRIVATE_WORKDIR: &str = private_workdir!();

/// Downloads a vendor's install script into a private directory, then runs it with `shell`.
/// Not `curl | sh`: a failed or cut download (`curl -f`, `set -e`) stops the install instead of
/// running half a script, or nothing at all and reporting success.
#[cfg(any(target_os = "linux", test))]
fn fetch_and_run(url: &str, shell: &str) -> String {
    format!("{PRIVATE_WORKDIR}; {}", fetch_into_workdir(url, shell))
}

/// [`fetch_and_run`] inside a script that already has its private directory `$d`.
#[cfg(any(target_os = "linux", test))]
fn fetch_into_workdir(url: &str, shell: &str) -> String {
    format!(r#"curl -fsSL '{url}' -o "$d/install.sh"; {shell} "$d/install.sh""#)
}

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
/// `uname -m` is x86_64/aarch64, matching the URLs. The bundle must carry a valid signature
/// (its `.sig`) by AWS's key, embedded here ([`AWS_CLI_KEY`]): gpgv, with only that key, must
/// report it by its fingerprint. Then `unzip -t` checks the archive is whole before anything
/// in it runs.
#[cfg(any(target_os = "linux", test))]
pub fn awscli_script() -> String {
    format!(
        r#"{PRIVATE_WORKDIR}; url="https://awscli.amazonaws.com/awscli-exe-linux-$(uname -m).zip"; curl -fsSL "$url" -o "$d/awscliv2.zip"; curl -fsSL "$url.sig" -o "$d/awscliv2.zip.sig"; {}; command -v unzip >/dev/null || {{ apt-get update && apt-get install -y unzip; }}; unzip -tq "$d/awscliv2.zip" >/dev/null; unzip -oq "$d/awscliv2.zip" -d "$d"; "$d/aws/install" --update"#,
        aws_verify()
    )
}

/// Stops the script unless `"$d/awscliv2.zip.sig"` is a valid signature of `"$d/awscliv2.zip"`
/// by [`AWS_CLI_KEY`].
#[cfg(any(target_os = "linux", test))]
fn aws_verify() -> String {
    let key = AWS_CLI_KEY.trim_end();
    format!(
        r#"{need_gpg}; cat > "$d/aws-cli.asc" <<'AWS_CLI_KEY'
{key}
AWS_CLI_KEY
gpg --batch --yes --dearmor --homedir "$d/gnupg" -o "$d/aws-cli.gpg" "$d/aws-cli.asc"; gpgv --homedir "$d/gnupg" --status-fd 1 --keyring "$d/aws-cli.gpg" "$d/awscliv2.zip.sig" "$d/awscliv2.zip" 2>/dev/null | grep -q '^\[GNUPG:\] VALIDSIG {AWS_CLI_KEY_FINGERPRINT} ' || {{ echo "The AWS CLI download isn't signed by AWS's key ({AWS_CLI_KEY_FINGERPRINT}). Nothing was installed." >&2; exit 1; }}"#,
        need_gpg = need_gpg!(),
    )
}

/// Microsoft's install script for Debian/Ubuntu, downloaded first then run. Like get.docker.com
/// it fetches Microsoft's repository key itself, so the key can't be held to a fingerprint here.
#[cfg(any(target_os = "linux", test))]
pub fn azurecli_script() -> String {
    fetch_and_run("https://aka.ms/InstallAzureCLIDeb", "bash")
}

/// Google's installer for the Cloud SDK, downloaded first then run, as the player (it installs
/// into `~/google-cloud-sdk`, whose bin the app adds to PATH; under pkexec it landed in /root).
/// Google's apt repository would be signed, but it installs system-wide as root, which the
/// per-user install and its removal (Cleanup) don't expect.
#[cfg(any(target_os = "linux", test))]
pub fn gcloud_script() -> String {
    format!(r#"{} --disable-prompts --install-dir="$HOME""#, fetch_and_run("https://sdk.cloud.google.com", "bash"))
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
        assert!(s.contains("get.docker.com") && s.contains("usermod -aG docker florianamette"), "{s}");
        // A name that isn't a plain login name never reaches the script.
        assert!(!docker_script(Some("a; rm -rf /")).contains("usermod"));
        assert!(!docker_script(None).contains("usermod"));
        assert!(valid_user("_svc-1.x") && !valid_user("Root") && !valid_user(""));
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

    #[test]
    fn repo_scripts_rerun_without_a_terminal_and_parse() {
        // gpg asked "overwrite?" on a second run, with no tty to answer it.
        for s in [TERRAFORM_SCRIPT, VIRTUALBOX_SCRIPT] {
            assert!(s.contains("gpg --batch --yes --dearmor"), "{s}");
            let ok = std::process::Command::new("sh").args(["-n", "-c", s]).status().unwrap();
            assert!(ok.success(), "not valid sh: {s}");
        }
        assert!(VIRTUALBOX_SCRIPT.contains("download.virtualbox.org/virtualbox/debian"));
    }

    #[test]
    fn root_scripts_never_use_fixed_temp_paths_or_pipe_downloads_into_a_shell() {
        // Under pkexec the AWS CLI was unzipped into /tmp/aws and /tmp/aws/install run as root:
        // any local user could plant that file first.
        for s in [awscli_script(), azurecli_script(), docker_script(Some("player")), TERRAFORM_SCRIPT.into(), VIRTUALBOX_SCRIPT.into()] {
            assert!(!s.contains("/tmp"), "fixed temp path: {s}");
            assert!(!s.contains("| sh") && !s.contains("| bash"), "download piped into a shell: {s}");
            assert!(!s.contains("curl -s") || s.contains("curl -fs"), "a failed download must fail: {s}");
            assert!(s.starts_with("set -e"), "{s}");
            let ok = std::process::Command::new("sh").args(["-n", "-c", &s]).status().unwrap();
            assert!(ok.success(), "not valid sh: {s}");
        }
        for s in [awscli_script(), azurecli_script(), docker_script(None)] {
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
        for fpr in [HASHICORP_KEY_FINGERPRINT, DOCKER_KEY_FINGERPRINT, AWS_CLI_KEY_FINGERPRINT] {
            assert!(fpr.len() == 40 && fpr.bytes().all(|b| b.is_ascii_digit() || (b'A'..=b'F').contains(&b)), "{fpr}");
        }
        // The key is checked, then installed for apt: never fetched straight into the keyring.
        let check = |s: &str, fpr: &str, install: &str| {
            let checked = s.find(&format!(r#"[ "$k" = "1 {fpr}" ]"#)).unwrap_or_else(|| panic!("no fingerprint check: {s}"));
            let installed = s.find(install).unwrap_or_else(|| panic!("no install of the key: {s}"));
            assert!(s.find(r#"-o "$d/key""#).is_some_and(|fetched| fetched < checked) && checked < installed, "{s}");
        };
        check(TERRAFORM_SCRIPT, HASHICORP_KEY_FINGERPRINT, "-o /usr/share/keyrings/hashicorp-archive-keyring.gpg");
        assert!(!TERRAFORM_SCRIPT.contains("gpg | gpg"), "{TERRAFORM_SCRIPT}");
        let docker = docker_script(None);
        check(&docker, DOCKER_KEY_FINGERPRINT, r#"install -m 0644 "$d/key" /etc/apt/keyrings/docker.asc"#);
        assert!(docker.contains("signed-by=/etc/apt/keyrings/docker.asc] $repo $codename stable"), "{docker}");
        // Elsewhere Docker's own script, downloaded first.
        assert!(docker.contains(r#"curl -fsSL 'https://get.docker.com' -o "$d/install.sh"; sh "$d/install.sh""#), "{docker}");
        for s in [TERRAFORM_SCRIPT.to_string(), docker] {
            assert!(s.contains(r#"--homedir "$d/gnupg""#) && !s.contains("--keyserver"), "{s}");
        }
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
        assert!(sh_ok(&s), "not valid sh: {s}");
    }

    #[test]
    fn gcloud_is_downloaded_then_run_as_the_player() {
        let s = gcloud_script();
        assert!(
            s.starts_with("set -e")
                && s.contains(r#"curl -fsSL 'https://sdk.cloud.google.com' -o "$d/install.sh"; bash "$d/install.sh" --disable-prompts --install-dir="$HOME""#),
            "{s}"
        );
        assert!(!s.contains("| bash") && !s.contains("| sh") && !s.contains("/tmp"), "{s}");
        assert!(sh_ok(&s), "not valid sh: {s}");
        for s in [docker_script(Some("player")), azurecli_script()] {
            assert!(sh_ok(&s), "not valid sh: {s}");
        }
    }

    /// Whether `sh -n` accepts `script` (no `sh` on Windows: not checked there).
    fn sh_ok(script: &str) -> bool {
        match std::process::Command::new("sh").args(["-n", "-c", script]).status() {
            Ok(status) => status.success(),
            Err(_) => cfg!(windows),
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_key_with_another_fingerprint_stops_the_install() {
        if !have("gpg") {
            eprintln!("no gpg here: the key check isn't run");
            return;
        }
        let pinned = concat!(r#"mkdir -p -m 700 "$d/gnupg"; "#, check_key!("FB5DB77FD5C118B80511ADA8A6310ACC4672475C", "AWS"), "; echo checked >&2");
        let (ok, err) = run_in_workdir(pinned, &[("key", AWS_CLI_KEY.as_bytes())]);
        assert!(ok && err.contains("checked"), "{err}");
        let other = concat!(r#"mkdir -p -m 700 "$d/gnupg"; "#, check_key!("9DC858229FC7DD38854AE2D88D81803C0EBFCD88", "Docker"), "; echo checked >&2");
        let (ok, err) = run_in_workdir(other, &[("key", AWS_CLI_KEY.as_bytes())]);
        assert!(!ok && !err.contains("checked"), "{err}");
        assert!(err.contains("Docker's signing key isn't the expected one (fingerprint 9DC858229FC7DD38854AE2D88D81803C0EBFCD88, got: 1 FB5DB77FD5C118B80511ADA8A6310ACC4672475C)"), "{err}");
        let (ok, err) = run_in_workdir(other, &[("key", b"<html>captive portal</html>")]);
        assert!(!ok && err.contains("got: 0 )"), "{err}");
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
    fn docker_picks_its_apt_repository_by_distro() {
        let dir = std::env::temp_dir().join(format!("cyberctf-os-release-{}-{}", std::process::id(), rand::random::<u32>()));
        std::fs::create_dir_all(&dir).unwrap();
        let release = dir.join("os-release");
        assert!(DOCKER_DIST.contains(". /etc/os-release;") && DOCKER_DIST.contains("command -v apt-get >/dev/null"));
        let probe = DOCKER_DIST.replace("/etc/os-release", &release.display().to_string()).replace("command -v apt-get >/dev/null", "true");
        let pick = |os: &str| {
            std::fs::write(&release, os).unwrap();
            let out = std::process::Command::new("sh").arg("-c").arg(format!("set -e; {probe}; echo \"$dist/$codename\"")).output().unwrap();
            assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
            String::from_utf8(out.stdout).unwrap().trim().to_string()
        };
        assert_eq!(pick("ID=ubuntu\nID_LIKE=debian\nVERSION_CODENAME=noble\nUBUNTU_CODENAME=noble\n"), "ubuntu/noble");
        assert_eq!(pick("ID=linuxmint\nID_LIKE=\"ubuntu debian\"\nVERSION_CODENAME=wilma\nUBUNTU_CODENAME=noble\n"), "ubuntu/noble");
        assert_eq!(pick("ID=debian\nVERSION_CODENAME=trixie\n"), "debian/trixie");
        assert_eq!(pick("ID=raspbian\nID_LIKE=debian\nVERSION_CODENAME=bookworm\n"), "raspbian/bookworm");
        assert_eq!(pick("ID=raspbian\nID_LIKE=debian\nVERSION_CODENAME=trixie\n"), "debian/trixie");
        // No repository of Docker's: its own script.
        assert_eq!(pick("ID=fedora\nVERSION_ID=43\n"), "/");
        assert_eq!(pick("ID=kali\nID_LIKE=debian\nVERSION_CODENAME=kali-rolling\n"), "/kali-rolling");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
