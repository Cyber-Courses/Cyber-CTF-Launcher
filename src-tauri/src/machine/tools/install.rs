//! Assisted, one-click install of lab dependencies, streaming the package manager's output.
//! It never shows a custom password prompt: it uses the OS's own trusted flow (winget's
//! UAC, Linux pkexec, or downloading the official app/installer for macOS casks and
//! letting the tool's own installer run).
//!
//! The guided setup as data: each tool's row ([`Dependency::row`]) says where it exists, what
//! proves it installed and where else the player gets it; its install ([`plan_on`]) and its
//! removal (`uninstall::plan_on`) are one exhaustive match per OS over the tool and the package
//! manager ([`Host`]), so a tool, an OS or a package manager added is a compile error until each
//! says what it does there. Every plan is built on any OS, so the tests read every row.

use serde::Deserialize;

use super::scripts::{
    awscli_script, azurecli_script, docker_dnf_script, docker_script, fetch_and_open, gcloud_script, libvirt_dnf_script, libvirt_script, qemu_dnf_script,
    qemu_script, terraform_dnf_script, terraform_script, vagrant_dnf_script, vagrant_script, virtualbox_dnf_script, virtualbox_script, vmware_utility_script,
};
use super::steps::{Step, command_line, run_step, run_steps, step};
use super::uninstall;
use crate::error::{Error, Result};
use crate::platform::host::{Arch, Host, LinuxPackages, Os, Packages};
use crate::platform::policy::{self, Key};
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

/// Where a tool exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Runs {
    /// On these OSes.
    On(&'static [Os]),
    /// Where this hypervisor does ([`Provider::row`]): the one table of which OS a hypervisor
    /// belongs to.
    As(Provider),
}

/// A program and the arguments it answers with its version.
pub(crate) type Probe = (&'static str, &'static [&'static str]);

/// A tool's row of the guided setup's table ([`Dependency::row`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ToolRow {
    /// Its name as the player reads it (Docker's on Linux is [`Dependency::name_on`]'s).
    pub(crate) name: &'static str,
    /// Where it exists.
    pub(crate) runs: Runs,
    /// The command that proves it installed, when there is one worth trusting (`wsl.exe` ships
    /// with Windows whether WSL is set up or not; UTM is an app; VMware's utility a service).
    pub(crate) probe: Option<Probe>,
    /// The app bundle that proves it installed on macOS, also before its CLI is on PATH.
    pub(crate) mac_app: Option<&'static str>,
    /// Its winget package, when winget installs and removes it on Windows.
    pub(crate) winget: Option<&'static str>,
    /// Where the player installs it themselves when the setup can't (a Linux distribution
    /// without apt or dnf).
    pub(crate) own_url: Option<&'static str>,
}

impl Dependency {
    /// Every tool the app installs.
    pub const ALL: [Dependency; 13] = [
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

    /// Its row of the guided setup's table.
    #[allow(clippy::too_many_lines, reason = "one row per tool: the table itself")]
    pub(crate) fn row(self) -> ToolRow {
        const ANY: Runs = Runs::On(&Os::ALL);
        /// A row with no macOS app.
        fn row(name: &'static str, runs: Runs, probe: Option<Probe>, winget: Option<&'static str>, own_url: Option<&'static str>) -> ToolRow {
            ToolRow { name, runs, probe, mac_app: None, winget, own_url }
        }
        match self {
            Dependency::Docker => ToolRow {
                mac_app: Some("/Applications/Docker.app"),
                ..row("Docker Desktop", ANY, Some(("docker", &["--version"])), Some("Docker.DockerDesktop"), Some("https://docs.docker.com/engine/install/"))
            },
            Dependency::Vagrant => {
                row("Vagrant", ANY, Some(("vagrant", &["--version"])), Some("Hashicorp.Vagrant"), Some("https://developer.hashicorp.com/vagrant/install"))
            }
            Dependency::Terraform => row("Terraform", ANY, Some(("terraform", &["version"])), Some("Hashicorp.Terraform"), None),
            Dependency::Virtualbox => ToolRow {
                mac_app: Some("/Applications/VirtualBox.app"),
                ..row(
                    Provider::Virtualbox.label(),
                    Runs::As(Provider::Virtualbox),
                    Some(("VBoxManage", &["--version"])),
                    Some("Oracle.VirtualBox"),
                    Some("https://www.virtualbox.org/wiki/Linux_Downloads"),
                )
            },
            Dependency::Qemu => {
                row(Provider::Qemu.label(), Runs::As(Provider::Qemu), Some(("qemu-img", &["--version"])), None, Some("https://www.qemu.org/download/"))
            }
            Dependency::Utm => ToolRow { mac_app: Some("/Applications/UTM.app"), ..row(Provider::Utm.label(), Runs::As(Provider::Utm), None, None, None) },
            Dependency::Libvirt => {
                row(Provider::Libvirt.label(), Runs::As(Provider::Libvirt), Some(("virsh", &["--version"])), None, Some("https://libvirt.org/"))
            }
            Dependency::Awscli => row("AWS CLI", ANY, Some(("aws", &["--version"])), Some("Amazon.AWSCLI"), None),
            Dependency::Azurecli => row("Azure CLI", ANY, Some(("az", &["version"])), Some("Microsoft.AzureCLI"), None),
            Dependency::Gcloud => row("Google Cloud CLI", ANY, Some(("gcloud", &["--version"])), Some("Google.CloudSDK"), None),
            Dependency::Wsl => row("WSL 2", Runs::On(&[Os::Windows]), None, None, None),
            Dependency::VmwareUtility => row("Vagrant VMware Utility", ANY, None, None, Some("https://developer.hashicorp.com/vagrant/install/vmware")),
            Dependency::Hyperv => row(Provider::Hyperv.label(), Runs::As(Provider::Hyperv), None, None, None),
        }
    }

    /// The tool the guided setup installs as hypervisor `provider`, where it does (the row that
    /// names it).
    pub(crate) fn of_hypervisor(provider: Provider) -> Option<Dependency> {
        Dependency::ALL.into_iter().find(|d| d.hypervisor() == Some(provider))
    }

    /// The hypervisor it is, when it is one.
    pub(crate) fn hypervisor(self) -> Option<Provider> {
        match self.row().runs {
            Runs::As(provider) => Some(provider),
            Runs::On(_) => None,
        }
    }

    /// Whether it exists on `os`: a hypervisor where [`Provider::runs_on`] says, the rest by its
    /// row. An install or a removal elsewhere is refused ([`not_here`]).
    pub fn runs_on(self, os: Os) -> bool {
        match self.row().runs {
            Runs::On(oses) => oses.contains(&os),
            Runs::As(provider) => provider.runs_on(os),
        }
    }

    /// Its name on this OS, as the setup installs it here (Docker Engine on Linux, from Docker's
    /// repository or the distribution's `docker.io`; Docker Desktop elsewhere): what a message and
    /// Settings' list of installed tools call it.
    pub fn name(self) -> &'static str {
        self.name_on(Os::HERE)
    }

    /// [`name`](Self::name) on `os`.
    pub(crate) fn name_on(self, os: Os) -> &'static str {
        match (self, os) {
            (Dependency::Docker, Os::Linux) => "Docker Engine",
            _ => self.row().name,
        }
    }
}

/// The install steps on macOS: Homebrew formulae, or a cask whose installer the user finishes.
fn macos_plan(dep: Dependency, host: Host) -> Result<Vec<Step>> {
    // HashiCorp's own package, not Homebrew's: needs no Homebrew.
    if dep == Dependency::VmwareUtility {
        return Ok(vec![super::vmware_utility::mac_install(host.arch)]);
    }
    let Some(Packages::Homebrew(brew)) = host.packages else {
        return Err(Error::Invalid("Homebrew is required. Install it from https://brew.sh, then try again.".into()));
    };
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
        Dependency::Libvirt | Dependency::Wsl | Dependency::Hyperv => return Err(not_here(dep, Os::Macos)),
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
        Dependency::VmwareUtility => vec![super::vmware_utility::mac_install(host.arch)],
    })
}

/// `winget <verb> <id>`, from the `winget` source alone. Without `--source`, winget asks every
/// source it has, the Microsoft Store's (`msstore`) too, and one that fails stops the command: on
/// a clean Windows 11 the Store source's certificate pinning failed (0x8a15005e, "The server
/// certificate did not match any of the expected values") and Docker Desktop's install with it.
/// Every package the setup installs is in the `winget` source.
pub(super) fn winget(verb: &str, id: &str, extra: &[&str]) -> Step {
    let mut args = vec![verb, "-e", "--id", id, "--source", "winget"];
    args.extend_from_slice(extra);
    step("winget", &args)
}

/// Why `dep` isn't installed on Windows on Arm, where the hypervisor it is or serves (VMware's
/// utility serves VMware Workstation) isn't built for it ([`Provider::row`]).
fn not_on_windows_on_arm(dep: Dependency, arch: Arch) -> Option<Error> {
    let provider = match dep {
        Dependency::VmwareUtility => Provider::VmwareDesktop,
        other => other.hypervisor()?,
    };
    (arch == Arch::Aarch64 && !provider.on_windows_on_arm()).then(|| {
        Error::Invalid(format!("{} doesn't run on Windows on Arm. Run VM labs with Hyper-V, on a server or in the cloud.", provider.label_on(Os::Windows)))
    })
}

/// The install step on Windows: a winget package, or WSL's, Hyper-V's and VMware's utility's
/// own elevated installers.
fn windows_plan(dep: Dependency, arch: Arch) -> Result<Vec<Step>> {
    if let Some(refusal) = not_on_windows_on_arm(dep, arch) {
        return Err(refusal);
    }
    if let Some(id) = dep.row().winget {
        return Ok(vec![winget("install", id, &["--accept-source-agreements", "--accept-package-agreements"])]);
    }
    match dep {
        // Turns on the WSL and Virtual Machine Platform features and installs the WSL 2 kernel.
        // Elevated through Windows' own UAC prompt; the features need a restart to take effect.
        // `-PassThru` surfaces wsl.exe's own exit code (without it PowerShell exits 0 whatever
        // happened); a declined UAC prompt throws, reported as ERROR_CANCELLED.
        Dependency::Wsl => Ok(vec![step(
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
        Dependency::Hyperv => Ok(vec![step("powershell", &["-NoProfile", "-Command", HYPERV_ON])]),
        Dependency::VmwareUtility => Ok(vec![super::vmware_utility::windows_install()]),
        Dependency::Utm | Dependency::Libvirt | Dependency::Qemu => Err(not_here(dep, Os::Windows)),
        Dependency::Docker
        | Dependency::Vagrant
        | Dependency::Terraform
        | Dependency::Virtualbox
        | Dependency::Awscli
        | Dependency::Azurecli
        | Dependency::Gcloud => Err(Error::Invalid(format!("{} has no winget package to install.", dep.name_on(Os::Windows)))),
    }
}

/// Turns Hyper-V on (Windows): refused on Windows Home, which has no Hyper-V; else DISM, elevated,
/// its "restart to finish" (3010) a success; a declined UAC prompt reported as `ERROR_CANCELLED`.
const HYPERV_ON: &str = "if ((Get-ItemProperty 'HKLM:\\SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion').EditionID -like 'Core*') { Write-Output 'This edition of Windows (Home) has no Hyper-V. Run VM labs with VirtualBox, on a server or in the cloud.'; exit 1 }; try { $p = Start-Process -FilePath dism.exe -ArgumentList '/Online','/Enable-Feature','/FeatureName:Microsoft-Hyper-V','/All','/NoRestart','/Quiet' -Verb RunAs -Wait -PassThru; if ($p.ExitCode -eq 3010) { exit 0 }; exit $p.ExitCode } catch { exit 1223 }";

/// `script` run as root through pkexec, which raises a graphical password prompt (polkit). pkexec
/// clears the environment, so the proxy variables set here (`proxy`, `NAME=value`) are handed to
/// the script: behind a corporate proxy apt and curl need them. Not on pkexec's command line,
/// which every local user can read (`ps`, `/proc/<pid>/cmdline`) while the prompt is up, a
/// proxy's password with it: in an owner-only file in a private folder of the player's, kept as
/// long as the step, which the script reads right after its prelude ([`read_proxy`]).
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
fn read_proxy(file: &std::path::Path) -> String {
    let names = crate::platform::proxy::VARS.map(|name| format!("{name}=*")).join("|");
    format!(r#"while IFS= read -r v; do case "$v" in {names}) export "$v" ;; esac; done < {}"#, crate::runtime::ssh::sh_quote(&file.to_string_lossy()))
}

/// `script` run as root through pkexec ([`as_root`]), with this process's proxy: every install
/// and removal step on Linux.
pub(super) fn root(script: RootScript) -> Result<Step> {
    as_root(script, &crate::platform::proxy::assignments(|name| std::env::var(name).ok()))
}

/// The install step on Linux, as root through pkexec, for the player `user`: apt and the vendors'
/// apt repositories, or dnf and the vendors' rpm repositories.
fn linux_plan(dep: Dependency, packages: Option<Packages>, user: Option<&str>) -> Result<Vec<Step>> {
    let Some(packages) = packages.and_then(Packages::linux) else {
        return Err(without_packages(dep));
    };
    let script = match (dep, packages) {
        (Dependency::Wsl | Dependency::Hyperv | Dependency::Utm, _) => return Err(not_here(dep, Os::Linux)),
        // From HashiCorp's apt repository, not the distribution's (older than labs need).
        (Dependency::Vagrant, LinuxPackages::Apt) => vagrant_script(),
        (Dependency::Vagrant, LinuxPackages::Dnf) => vagrant_dnf_script(),
        (Dependency::Docker, LinuxPackages::Apt) => docker_script(user),
        (Dependency::Docker, LinuxPackages::Dnf) => docker_dnf_script(user),
        (Dependency::Terraform, LinuxPackages::Apt) => terraform_script(),
        (Dependency::Terraform, LinuxPackages::Dnf) => terraform_dnf_script(),
        // Fedora: RPM Fusion's VirtualBox, the distribution's libvirt and QEMU.
        (Dependency::Virtualbox, LinuxPackages::Apt) => virtualbox_script(),
        (Dependency::Virtualbox, LinuxPackages::Dnf) => virtualbox_dnf_script(user),
        // The player joins /dev/kvm's group: the labs' VMs boot with KVM.
        (Dependency::Qemu, LinuxPackages::Apt) => qemu_script(user),
        (Dependency::Qemu, LinuxPackages::Dnf) => qemu_dnf_script(user),
        (Dependency::Libvirt, LinuxPackages::Apt) => libvirt_script(user),
        (Dependency::Libvirt, LinuxPackages::Dnf) => libvirt_dnf_script(user),
        // AWS CLI v2 from Amazon's signed bundle, in a private temporary directory: no package
        // manager's.
        (Dependency::Awscli, LinuxPackages::Apt | LinuxPackages::Dnf) => awscli_script(),
        // Azure CLI and gcloud from Microsoft's and Google's apt repositories, their keys pinned;
        // through apt only for now, as VMware's utility.
        (Dependency::Azurecli, LinuxPackages::Apt) => azurecli_script(),
        (Dependency::Gcloud, LinuxPackages::Apt) => gcloud_script(),
        (Dependency::VmwareUtility, LinuxPackages::Apt) => vmware_utility_script(),
        (Dependency::Azurecli | Dependency::Gcloud | Dependency::VmwareUtility, LinuxPackages::Dnf) => return Err(apt_only(dep)),
    };
    Ok(vec![root(script)?])
}

/// What an install of `dep` says on a Linux distribution with neither apt nor dnf: the guided
/// setup doesn't install there yet, so the player installs it themselves; instead of a command
/// that fails.
pub(crate) fn without_packages(dep: Dependency) -> Error {
    Error::Invalid(format!(
        "This distribution isn't supported by the guided setup yet (it has neither apt nor dnf): install {} yourself, then re-check.",
        dep.name_on(Os::Linux)
    ))
}

/// What an install of `dep` says on a dnf distribution for a tool the guided setup installs
/// through apt only for now (the cloud CLIs, VMware's utility).
pub(crate) fn apt_only(dep: Dependency) -> Error {
    Error::Invalid(format!("The guided setup installs {} through apt only for now: install it yourself, then re-check.", dep.name_on(Os::Linux)))
}

/// The ordered install steps for a dependency on `host`, for the player `user` (Linux joins
/// them to the groups a tool's devices need).
pub(super) fn plan_on(dep: Dependency, host: Host, user: Option<&str>) -> Result<Vec<Step>> {
    if !dep.runs_on(host.os) {
        return Err(not_here(dep, host.os));
    }
    match host.os {
        Os::Macos => macos_plan(dep, host),
        Os::Windows => windows_plan(dep, host.arch),
        Os::Linux => linux_plan(dep, host.packages, user),
    }
}

/// The ordered install steps for a dependency on this computer.
fn plan(dep: Dependency) -> Result<Vec<Step>> {
    plan_on(dep, Host::here(), crate::platform::exec::login_name().as_deref())
}

/// Why `dep` can't be installed or removed on `os`: it doesn't run there ([`Dependency::runs_on`]).
pub(super) fn not_here(dep: Dependency, os: Os) -> Error {
    Error::Invalid(format!("{} doesn't run on {}: there is nothing of it to install or remove here.", dep.name_on(os), os.name()))
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
