//! Assisted, one-click install of lab dependencies, streaming the package manager's output.
//! It never shows a custom password prompt: it uses the OS's own trusted flow (winget's
//! UAC, Linux pkexec, or downloading the official app/installer for macOS casks and
//! letting the tool's own installer run).

use serde::Deserialize;

#[cfg(target_os = "macos")]
use super::scripts::fetch_and_open;
#[cfg(target_os = "linux")]
use super::scripts::{
    awscli_script, azurecli_script, docker_dnf_script, docker_script, gcloud_script, libvirt_dnf_script, libvirt_script, qemu_dnf_script, qemu_script,
    terraform_dnf_script, terraform_script, vagrant_dnf_script, vagrant_script, virtualbox_dnf_script, virtualbox_script, vmware_utility_script,
};
#[cfg(any(target_os = "macos", windows))]
use super::steps::step;
use super::steps::{Step, command_line, run_step, run_steps};
use super::uninstall;
use crate::error::{Error, Result};
use crate::platform::policy::{self, Key};
#[cfg(any(target_os = "linux", test))]
use crate::platform::root_script::RootScript;
use crate::runtime::providers::Provider;

/// A tool the guided setup installs, as the webview names it.
#[derive(Deserialize, serde::Serialize, Clone, Copy, PartialEq, Eq, Debug, ts_rs::TS)]
#[serde(rename_all = "lowercase")]
pub enum Dependency {
    /// Docker: Docker Desktop on macOS and Windows, Docker Engine on Linux.
    Docker,
    /// HashiCorp's Vagrant.
    Vagrant,
    /// HashiCorp's Terraform.
    Terraform,
    /// Oracle's VirtualBox.
    Virtualbox,
    /// QEMU.
    Qemu,
    /// UTM (macOS).
    Utm,
    /// libvirt, with what the vagrant-libvirt plugin builds against (Linux).
    Libvirt,
    /// The AWS CLI.
    Awscli,
    /// The Azure CLI.
    Azurecli,
    /// The Google Cloud CLI.
    Gcloud,
    /// Windows Subsystem for Linux (WSL 2), enabled through its own installer.
    Wsl,
    /// HashiCorp's Vagrant VMware Utility, the service VMware's Vagrant plugin drives VMware
    /// through: from HashiCorp's signed package (see `vmware_utility`).
    #[serde(rename = "vmware-utility")]
    VmwareUtility,
    /// Windows' Hyper-V feature, turned on through Windows' own servicing tool (DISM): Windows
    /// Pro, Enterprise and Education have it, and it starts at the next restart.
    Hyperv,
}

impl Dependency {
    /// Whether it exists on `os` (`std::env::consts::OS`): a hypervisor where
    /// [`Provider::runs_on`] says, WSL on Windows,
    /// every other tool anywhere. An install or a removal elsewhere is refused ([`not_here`]).
    pub fn runs_on(self, os: &str) -> bool {
        match self {
            Dependency::Wsl => os == "windows",
            Dependency::Utm => Provider::Utm.runs_on(os),
            Dependency::Libvirt => Provider::Libvirt.runs_on(os),
            Dependency::Hyperv => Provider::Hyperv.runs_on(os),
            Dependency::Qemu => Provider::Qemu.runs_on(os),
            _ => true,
        }
    }

    /// Its name on this OS, as the setup installs it here (Docker Engine on Linux, from Docker's
    /// repository or the distribution's `docker.io`; Docker Desktop elsewhere): what a message and Settings' list of installed
    /// tools call it.
    pub fn name(self) -> &'static str {
        self.name_on(std::env::consts::OS)
    }

    /// [`name`](Self::name) on `os`.
    fn name_on(self, os: &str) -> &'static str {
        match self {
            Dependency::Docker if os == "linux" => "Docker Engine",
            Dependency::Docker => "Docker Desktop",
            Dependency::Vagrant => "Vagrant",
            Dependency::Terraform => "Terraform",
            Dependency::Virtualbox => Provider::Virtualbox.label(),
            Dependency::Qemu => Provider::Qemu.label(),
            Dependency::Utm => Provider::Utm.label(),
            Dependency::Libvirt => Provider::Libvirt.label(),
            Dependency::Awscli => "AWS CLI",
            Dependency::Azurecli => "Azure CLI",
            Dependency::Gcloud => "Google Cloud CLI",
            Dependency::Wsl => "WSL 2",
            Dependency::VmwareUtility => "Vagrant VMware Utility",
            Dependency::Hyperv => Provider::Hyperv.label(),
        }
    }
}

#[cfg(test)]
impl Dependency {
    /// Every tool the app installs.
    pub(super) const ALL: [Dependency; 13] = [
        Dependency::Docker,
        Dependency::Vagrant,
        Dependency::Terraform,
        Dependency::Virtualbox,
        Dependency::Qemu,
        Dependency::Utm,
        Dependency::Libvirt,
        Dependency::Awscli,
        Dependency::Azurecli,
        Dependency::Gcloud,
        Dependency::Wsl,
        Dependency::VmwareUtility,
        Dependency::Hyperv,
    ];
}

/// The install steps on macOS: Homebrew formulae, or a cask whose installer the user finishes.
#[cfg(target_os = "macos")]
fn macos_plan(dep: Dependency) -> Result<Vec<Step>> {
    // HashiCorp's own package, not Homebrew's: needs no Homebrew.
    if dep == Dependency::VmwareUtility {
        return Ok(vec![super::vmware_utility::mac_install(std::env::consts::ARCH)]);
    }
    let brew = crate::platform::brew_bin().ok_or_else(|| Error::Invalid("Homebrew is required. Install it from https://brew.sh, then try again.".into()))?;
    Ok(match dep {
        // Docker Desktop: download the app with brew, then open its installer; the user
        // finishes in Docker's own flow (includes the docker CLI + compose).
        Dependency::Docker => vec![fetch_and_open(brew, "docker-desktop", "Docker Desktop")],
        // Admin-requiring: download with brew, then open the tool's native installer.
        Dependency::Vagrant => vec![fetch_and_open(brew, "vagrant", "Vagrant")],
        // Terraform is a brew formula (no admin); full tap name since core dropped it.
        Dependency::Terraform => vec![step(brew, &["install", "hashicorp/tap/terraform"])],
        Dependency::Virtualbox => vec![fetch_and_open(brew, "virtualbox", "VirtualBox")],
        // QEMU is a brew formula (no admin); UTM is a cask with its own installer.
        // Update Homebrew first so an outdated one learns this macOS (it otherwise refuses every
        // install with "unknown or unsupported macOS version: :dunno"), but never let that update
        // block the install: it runs best-effort, then the install runs with auto-update off. A
        // Mac whose Homebrew is already current installs even when its update lock is stuck, and a
        // real problem (an outdated Homebrew or Xcode, a root-owned lock) surfaces from the install
        // with a readable fix rather than a raw brew error (see steps::brew_trouble).
        Dependency::Qemu => vec![Step {
            program: "sh".into(),
            args: vec![
                "-c".into(),
                format!("'{brew}' update || echo 'Homebrew was not updated; continuing with the install.'\nHOMEBREW_NO_AUTO_UPDATE=1 '{brew}' install qemu"),
            ],
            note: Some("Installing QEMU (updating Homebrew first if it is out of date for this macOS).".into()),
            private: None,
        }],
        Dependency::Utm => vec![fetch_and_open(brew, "utm", "UTM")],
        Dependency::Libvirt | Dependency::Wsl | Dependency::Hyperv => return Err(not_here(dep)),
        // Cloud CLIs (brew formulae; gcloud is a cask).
        Dependency::Awscli => vec![step(brew, &["install", "awscli"])],
        // Microsoft's prebuilt cask instead of the `azure-cli` formula: on recent macOS the
        // formula has no bottle and builds its whole tree (llvm, rust) from source, which takes
        // ~an hour. The cask is a signed download. `trust` is needed because the tap is third-party.
        // https://learn.microsoft.com/cli/azure/install-azure-cli-macos
        Dependency::Azurecli => vec![
            step(brew, &["tap", "azure/azure-cli"]),
            // `brew trust` exists from Homebrew 7 only; an older Homebrew needs no trust and
            // must not fail here ("Unknown command"). On 7, an untrusted tap still fails at
            // the install step with Homebrew's own message.
            Step {
                program: "sh".into(),
                args: vec!["-c".into(), format!("'{brew}' trust azure/azure-cli 2>/dev/null || true")],
                note: Some("Trusting the azure/azure-cli tap (Homebrew 7 asks for it; older versions don't).".into()),
                private: None,
            },
            Step {
                program: brew.to_string(),
                args: vec!["install".into(), "--cask".into(), "azure-cli-preview".into()],
                note: Some("Installing the prebuilt Azure CLI cask (avoids a long source build on this macOS).".into()),
                private: None,
            },
        ],
        Dependency::Gcloud => vec![step(brew, &["install", "--cask", "google-cloud-sdk"])],
        Dependency::VmwareUtility => vec![super::vmware_utility::mac_install(std::env::consts::ARCH)],
    })
}

/// `winget <verb> <id>`, from the `winget` source alone. Without `--source`, winget asks every
/// source it has, the Microsoft Store's (`msstore`) too, and one that fails stops the command: on
/// a clean Windows 11 the Store source's certificate pinning failed (0x8a15005e, "The server
/// certificate did not match any of the expected values") and Docker Desktop's install with it.
/// Every package the setup installs is in the `winget` source.
#[cfg(any(target_os = "windows", test))]
pub(super) fn winget(verb: &str, id: &str, extra: &[&str]) -> Step {
    let mut args = vec![verb, "-e", "--id", id, "--source", "winget"];
    args.extend_from_slice(extra);
    super::steps::step("winget", &args)
}

/// The winget package of `dep`, when winget installs it on Windows.
#[cfg(target_os = "windows")]
pub(super) fn winget_package(dep: Dependency) -> Option<&'static str> {
    Some(match dep {
        Dependency::Docker => "Docker.DockerDesktop",
        Dependency::Vagrant => "Hashicorp.Vagrant",
        Dependency::Terraform => "Hashicorp.Terraform",
        Dependency::Virtualbox => "Oracle.VirtualBox",
        Dependency::Qemu => "SoftwareFreedomConservancy.QEMU",
        Dependency::Awscli => "Amazon.AWSCLI",
        Dependency::Azurecli => "Microsoft.AzureCLI",
        Dependency::Gcloud => "Google.CloudSDK",
        Dependency::Utm | Dependency::Libvirt | Dependency::Wsl | Dependency::VmwareUtility | Dependency::Hyperv => return None,
    })
}

/// The install step on Windows: a winget package, or WSL's own elevated installer.
#[cfg(target_os = "windows")]
fn windows_plan(dep: Dependency) -> Result<Vec<Step>> {
    if dep == Dependency::Virtualbox && std::env::consts::ARCH == "aarch64" {
        return Err(Error::Invalid("VirtualBox doesn't run on Windows on Arm. Run VM labs with Hyper-V, on a server or in the cloud.".into()));
    }
    match (dep, winget_package(dep)) {
        (_, Some(id)) => Ok(vec![winget("install", id, &["--accept-source-agreements", "--accept-package-agreements"])]),
        // Turns on the WSL and Virtual Machine Platform features and installs the WSL 2 kernel.
        // Elevated through Windows' own UAC prompt; the features need a restart to take effect.
        // `-PassThru` surfaces wsl.exe's own exit code (without it PowerShell exits 0 whatever
        // happened); a declined UAC prompt throws, reported as ERROR_CANCELLED.
        (Dependency::Wsl, None) => Ok(vec![step(
            "powershell",
            &[
                "-NoProfile",
                "-Command",
                "try { $p = Start-Process -FilePath wsl.exe -ArgumentList '--install','--no-distribution' -Verb RunAs -Wait -PassThru; exit $p.ExitCode } catch { exit 1223 }",
            ],
        )]),
        // Turns the Hyper-V feature on with Windows' own servicing tool, elevated through Windows'
        // UAC prompt: DISM's 3010 is "done, restart to finish". Windows Home has no Hyper-V: said
        // before anything runs.
        (Dependency::Hyperv, None) => Ok(vec![step("powershell", &["-NoProfile", "-Command", HYPERV_ON])]),

        // VMware Workstation doesn't run on Windows on Arm, so neither does its utility.
        (Dependency::VmwareUtility, None) if std::env::consts::ARCH == "aarch64" => {
            Err(Error::Invalid("VMware Workstation doesn't run on Windows on Arm. Run VM labs with Hyper-V, on a server or in the cloud.".into()))
        }
        (Dependency::VmwareUtility, None) => Ok(vec![super::vmware_utility::windows_install()]),
        (_, None) => Err(not_here(dep)),
    }
}

/// Turns Hyper-V on (Windows): refused on Windows Home, which has no Hyper-V; else DISM, elevated,
/// its "restart to finish" (3010) a success; a declined UAC prompt reported as `ERROR_CANCELLED`.
#[cfg(any(target_os = "windows", test))]
const HYPERV_ON: &str = "if ((Get-ItemProperty 'HKLM:\\SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion').EditionID -like 'Core*') { Write-Output 'This edition of Windows (Home) has no Hyper-V. Run VM labs with VirtualBox, on a server or in the cloud.'; exit 1 }; try { $p = Start-Process -FilePath dism.exe -ArgumentList '/Online','/Enable-Feature','/FeatureName:Microsoft-Hyper-V','/All','/NoRestart','/Quiet' -Verb RunAs -Wait -PassThru; if ($p.ExitCode -eq 3010) { exit 0 }; exit $p.ExitCode } catch { exit 1223 }";

/// `script` run as root through pkexec, which raises a graphical password prompt (polkit). pkexec
/// clears the environment, so the proxy variables set here (`proxy`, `NAME=value`) are handed to
/// the script: behind a corporate proxy apt and curl need them. Not on pkexec's command line,
/// which every local user can read (`ps`, `/proc/<pid>/cmdline`) while the prompt is up, a
/// proxy's password with it: in an owner-only file in a private folder of the player's, kept as
/// long as the step, which the script reads right after its prelude ([`read_proxy`]).
#[cfg(any(target_os = "linux", test))]
fn as_root(script: RootScript, proxy: &[String]) -> Result<Step> {
    // Through pkexec, or unattended through sudo (see `platform::elevation`).
    let root = |script: String| {
        let (program, args) = crate::platform::elevation::as_root("env", &["sh".into(), "-c".into(), script]);
        Step { program, args, note: None, private: None }
    };
    // A value over two lines can't be one line of the file; no proxy has one.
    let proxy: Vec<&str> = proxy.iter().map(String::as_str).filter(|a| !a.contains(['\n', '\r'])).collect();
    if proxy.is_empty() {
        return Ok(root(script.into_string()));
    }
    // Owner-only from its creation, under a name no one can take first.
    let mut private = tempfile::Builder::new();
    private.prefix("cyberctf-root-");
    #[cfg(unix)]
    private.permissions(std::os::unix::fs::PermissionsExt::from_mode(0o700));
    let dir = private.tempdir()?;
    let file = dir.path().join("proxy");
    let mut lines = proxy.join("\n");
    lines.push('\n');
    crate::platform::private_fs::write(&file, lines)?;
    let mut step = root(script.with_after_prelude(&read_proxy(&file)));
    step.private = Some(dir);
    Ok(step)
}

/// The shell that exports the proxy variables `file` lists (`NAME=value`, one a line), read as
/// data: a line naming anything but a proxy variable is skipped, and nothing in a value is run.
#[cfg(any(target_os = "linux", test))]
fn read_proxy(file: &std::path::Path) -> String {
    let names = crate::platform::proxy::VARS.map(|name| format!("{name}=*")).join("|");
    format!(r#"while IFS= read -r v; do case "$v" in {names}) export "$v" ;; esac; done < {}"#, crate::runtime::ssh::sh_quote(&file.to_string_lossy()))
}

/// `script` run as root through pkexec ([`as_root`]), with this process's proxy: every install
/// and removal step on Linux.
#[cfg(target_os = "linux")]
pub(super) fn root(script: RootScript) -> Result<Step> {
    as_root(script, &crate::platform::proxy::assignments(|name| std::env::var(name).ok()))
}

/// The install steps on Linux: apt and the vendors' apt repositories, as root through pkexec.
#[cfg(target_os = "linux")]
fn linux_plan(dep: Dependency) -> Result<Vec<Step>> {
    linux_plan_with(dep, packages_here())
}

/// The package manager a Linux install goes through: apt (Debian, Ubuntu and the distributions
/// built on them), dnf (Fedora and those built on it), or none the guided setup knows.
#[cfg(target_os = "linux")]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Packages {
    /// apt-get: Debian, Ubuntu and the distributions built on them.
    Apt,
    /// dnf: Fedora and the distributions built on it.
    Dnf,
    /// Neither: the player installs each tool themselves (openSUSE, Arch, …).
    None,
}

/// [`linux_plan`] on a machine with that package manager.
#[cfg(target_os = "linux")]
fn linux_plan_with(dep: Dependency, packages: Packages) -> Result<Vec<Step>> {
    let user = crate::platform::exec::login_name();
    let user = user.as_deref();
    let script = match (dep, packages) {
        (Dependency::Wsl | Dependency::Hyperv | Dependency::Utm, _) => return Err(not_here(dep)),
        (_, Packages::None) => return Err(without_packages(dep)),
        // Fedora: the vendors' rpm repositories (Docker's, HashiCorp's), RPM Fusion's VirtualBox,
        // the distribution's libvirt and QEMU; the cloud CLIs and VMware's utility through apt only
        // for now.
        (Dependency::Docker, Packages::Dnf) => docker_dnf_script(user),
        (Dependency::Vagrant, Packages::Dnf) => vagrant_dnf_script(),
        (Dependency::Terraform, Packages::Dnf) => terraform_dnf_script(),
        (Dependency::Virtualbox, Packages::Dnf) => virtualbox_dnf_script(user),
        (Dependency::Qemu, Packages::Dnf) => qemu_dnf_script(user),
        (Dependency::Libvirt, Packages::Dnf) => libvirt_dnf_script(user),
        // AWS CLI v2 from Amazon's signed bundle, in a private temporary directory: no package
        // manager's.
        (Dependency::Awscli, Packages::Apt | Packages::Dnf) => awscli_script(),
        (_, Packages::Dnf) => return Err(apt_only(dep)),
        (Dependency::Docker, Packages::Apt) => docker_script(user),
        // From HashiCorp's apt repository, not the distribution's (older than labs need).
        (Dependency::Vagrant, Packages::Apt) => vagrant_script(),
        // Terraform via HashiCorp's official apt repo (best effort across Debian/Ubuntu).
        (Dependency::Terraform, Packages::Apt) => terraform_script(),
        (Dependency::Virtualbox, Packages::Apt) => virtualbox_script(),
        // The player joins /dev/kvm's group: the labs' VMs boot with KVM.
        (Dependency::Qemu, Packages::Apt) => qemu_script(user),
        (Dependency::Libvirt, Packages::Apt) => libvirt_script(user),
        // Azure CLI and gcloud from Microsoft's and Google's apt repositories, their keys pinned.
        (Dependency::Azurecli, Packages::Apt) => azurecli_script(),
        (Dependency::Gcloud, Packages::Apt) => gcloud_script(),
        (Dependency::VmwareUtility, Packages::Apt) => vmware_utility_script(),
    };
    Ok(vec![root(script)?])
}

/// The package manager on this machine's PATH: apt-get, else dnf, else none the guided setup
/// installs through (openSUSE's zypper, Arch's pacman: the player installs themselves).
#[cfg(target_os = "linux")]
pub(super) fn packages_here() -> Packages {
    if crate::platform::on_path("apt-get") {
        Packages::Apt
    } else if crate::platform::on_path("dnf") {
        Packages::Dnf
    } else {
        Packages::None
    }
}

/// What an install of `dep` says on a Linux distribution with neither apt nor dnf: the guided
/// setup doesn't install there yet, so the player installs it themselves; instead of a command
/// that fails.
#[cfg(any(target_os = "linux", test))]
pub(crate) fn without_packages(dep: Dependency) -> Error {
    Error::Invalid(format!(
        "This distribution isn't supported by the guided setup yet (it has neither apt nor dnf): install {} yourself, then re-check.",
        dep.name()
    ))
}

/// What an install of `dep` says on a dnf distribution for a tool the guided setup installs
/// through apt only for now (the cloud CLIs, VMware's utility).
#[cfg(target_os = "linux")]
pub(crate) fn apt_only(dep: Dependency) -> Error {
    Error::Invalid(format!("The guided setup installs {} through apt only for now: install it yourself, then re-check.", dep.name()))
}

/// The ordered install steps for a dependency on this OS.
fn plan(dep: Dependency) -> Result<Vec<Step>> {
    if !dep.runs_on(std::env::consts::OS) {
        return Err(not_here(dep));
    }
    #[cfg(target_os = "macos")]
    {
        macos_plan(dep)
    }
    #[cfg(target_os = "windows")]
    {
        windows_plan(dep)
    }
    #[cfg(target_os = "linux")]
    {
        linux_plan(dep)
    }
}

/// Why `dep` can't be installed or removed on this OS: it doesn't run here ([`Dependency::runs_on`]).
pub(super) fn not_here(dep: Dependency) -> Error {
    let os = crate::platform::os_name(std::env::consts::OS);
    Error::Invalid(format!("{} doesn't run on {os}: there is nothing of it to install or remove here.", dep.name()))
}

/// Refused when this computer's policy turns one-click installs off (`DisableToolInstalls`): the
/// organization installs its tools itself. Said on the install's log too.
pub(super) fn installs_allowed(on_line: &mut impl FnMut(String)) -> Result<()> {
    policy::current().check(Key::DisableToolInstalls).inspect_err(|e| on_line(e.to_string()))
}

/// Installs a dependency, streaming each command and its output to the UI.
pub async fn install_dependency(dirs: crate::platform::AppDirs, dependency: Dependency, mut on_line: impl FnMut(String)) -> Result<()> {
    installs_allowed(&mut on_line)?;
    let steps = plan(dependency).inspect_err(|e| on_line(e.to_string()))?;
    let before = uninstall::present(dependency).await;
    run_steps(&steps, &mut on_line).await?;
    // Windows: the installer wrote its folder to the registry PATH; pick it up before looking.
    crate::platform::env_path::refresh();
    // Remembered so Cleanup can offer to remove what Cyber CTF installed, and only that: a tool
    // that was already there is never recorded, and one whose installer was only opened (macOS
    // casks) or isn't found yet counts once a later check finds it.
    if let Some(before) = before {
        let after = uninstall::present(dependency).await.unwrap_or(false);
        uninstall::record_outcome(&dirs, dependency, uninstall::outcome(before, after));
    }
    // Its features start at the next restart: the check asks for one until then, even if the
    // app is reopened first.
    if dependency == Dependency::Wsl {
        crate::machine::system::wsl::mark_installed(&dirs);
    }
    if dependency == Dependency::Hyperv {
        crate::machine::system::wsl::mark_hyperv_on(&dirs);
    }
    on_line("Done. Re-checking this machine…".into());
    Ok(())
}

/// A Vagrant plugin name: only ever a `vagrant-*` / `vagrant_*` identifier.
fn valid_plugin(plugin: &str) -> bool {
    (plugin.starts_with("vagrant-") || plugin.starts_with("vagrant_"))
        && plugin.len() <= 64
        && plugin.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Installs a Vagrant plugin (userland, no admin), streaming the output. The plugin name
/// is validated so it can never be anything but a `vagrant-*` / `vagrant_*` identifier.
pub async fn install_vagrant_plugin(plugin: String, mut on_line: impl FnMut(String)) -> Result<()> {
    installs_allowed(&mut on_line)?;
    if !valid_plugin(&plugin) {
        return Err(Error::Invalid(format!("invalid plugin name `{plugin}`")));
    }
    let step = Step { program: "vagrant".into(), args: vec!["plugin".into(), "install".into(), plugin], note: None, private: None };
    on_line(command_line(&step));
    run_step(&step, &mut on_line).await?;
    on_line("Done. Re-checking this machine…".into());
    Ok(())
}

#[cfg(test)]
mod tests;
