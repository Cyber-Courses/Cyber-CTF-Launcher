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

/// HashiCorp's apt repo, then terraform. The codename is the distro's as the vendors' repos name
/// it (an Ubuntu derivative like Mint uses its Ubuntu base's). `--batch --yes`: a second run (after a partial one)
/// overwrites the keyring instead of asking on a terminal it doesn't have.
#[cfg(any(target_os = "linux", test))]
pub const TERRAFORM_SCRIPT: &str = concat!(
    "set -e; ",
    r#"codename="$(. /etc/os-release && echo "${UBUNTU_CODENAME:-$VERSION_CODENAME}")"; [ -n "$codename" ] || codename="$(lsb_release -cs)""#,
    "; curl -fsSL https://apt.releases.hashicorp.com/gpg | gpg --batch --yes --dearmor -o /usr/share/keyrings/hashicorp-archive-keyring.gpg",
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

/// Docker's convenience script, then the player into the `docker` group so the CLI can reach
/// the daemon without root (pkexec runs as root, so `$USER` there is root: the name comes from
/// the app). The group applies from the next login.
#[cfg(any(target_os = "linux", test))]
pub fn docker_script(user: Option<&str>) -> String {
    let mut s = fetch_and_run("https://get.docker.com", "sh");
    if let Some(u) = user.filter(|u| valid_user(u)) {
        s.push_str(&format!("; usermod -aG docker {u}; echo 'Added {u} to the docker group: log out and back in (or restart) for it to apply.'"));
    }
    s
}

/// The start of every script run as root that downloads something: stop at the first failure,
/// and work in a fresh private directory (`mktemp -d`, mode 0700, removed on exit). A fixed path
/// such as `/tmp/aws` could be prepared by any local user beforehand and then run as root.
#[cfg(any(target_os = "linux", test))]
const PRIVATE_WORKDIR: &str = r#"set -e; d="$(mktemp -d)"; trap 'rm -rf "$d"' EXIT"#;

/// Downloads a vendor's install script into a private directory, then runs it with `shell`.
/// Not `curl | sh`: a failed or cut download (`curl -f`, `set -e`) stops the install instead of
/// running half a script, or nothing at all and reporting success.
#[cfg(any(target_os = "linux", test))]
fn fetch_and_run(url: &str, shell: &str) -> String {
    format!(r#"{PRIVATE_WORKDIR}; curl -fsSL '{url}' -o "$d/install.sh"; {shell} "$d/install.sh""#)
}

/// AWS CLI v2 from Amazon's official bundle, unpacked and installed from a private directory.
/// The distro `awscli` package is v1, which has no `aws login` (the app needs v2 >= 2.32).
/// `uname -m` is x86_64/aarch64, matching the URLs. `unzip -t` checks the archive is whole
/// before anything in it runs.
#[cfg(any(target_os = "linux", test))]
pub fn awscli_script() -> String {
    format!(
        r#"{PRIVATE_WORKDIR}; curl -fsSL "https://awscli.amazonaws.com/awscli-exe-linux-$(uname -m).zip" -o "$d/awscliv2.zip"; command -v unzip >/dev/null || {{ apt-get update && apt-get install -y unzip; }}; unzip -tq "$d/awscliv2.zip" >/dev/null; unzip -oq "$d/awscliv2.zip" -d "$d"; "$d/aws/install" --update"#
    )
}

/// Microsoft's install script for Debian/Ubuntu, downloaded first then run.
#[cfg(any(target_os = "linux", test))]
pub fn azurecli_script() -> String {
    fetch_and_run("https://aka.ms/InstallAzureCLIDeb", "bash")
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
}
