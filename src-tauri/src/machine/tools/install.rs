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

use super::steps::{Plan, run_plan, run_step};
use super::uninstall;
use crate::error::{Error, Result};
use crate::platform::host::{Arch, Host, Os, Packages};
use crate::platform::policy::{self, Key};
use crate::runtime::providers::Provider;
use isothrum_machine::install::{Elevation, Refusal, linux};

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

    /// The same tool as isothrum-machine names it.
    pub(crate) fn machine(self) -> isothrum_machine::install::Dependency {
        use isothrum_machine::install::Dependency as Machine;
        match self {
            Dependency::Docker => Machine::Docker,
            Dependency::Vagrant => Machine::Vagrant,
            Dependency::Terraform => Machine::Terraform,
            Dependency::Virtualbox => Machine::Virtualbox,
            Dependency::Qemu => Machine::Qemu,
            Dependency::Utm => Machine::Utm,
            Dependency::Libvirt => Machine::Libvirt,
            Dependency::Awscli => Machine::Awscli,
            Dependency::Azurecli => Machine::Azurecli,
            Dependency::Gcloud => Machine::Gcloud,
            Dependency::Wsl => Machine::Wsl,
            Dependency::VmwareUtility => Machine::VmwareUtility,
            Dependency::Hyperv => Machine::Hyperv,
        }
    }

    /// The tool isothrum-machine names `machine`.
    pub(crate) fn of_machine(machine: isothrum_machine::install::Dependency) -> Dependency {
        use isothrum_machine::install::Dependency as Machine;
        match machine {
            Machine::Docker => Dependency::Docker,
            Machine::Vagrant => Dependency::Vagrant,
            Machine::Terraform => Dependency::Terraform,
            Machine::Virtualbox => Dependency::Virtualbox,
            Machine::Qemu => Dependency::Qemu,
            Machine::Utm => Dependency::Utm,
            Machine::Libvirt => Dependency::Libvirt,
            Machine::Awscli => Dependency::Awscli,
            Machine::Azurecli => Dependency::Azurecli,
            Machine::Gcloud => Dependency::Gcloud,
            Machine::Wsl => Dependency::Wsl,
            Machine::VmwareUtility => Dependency::VmwareUtility,
            Machine::Hyperv => Dependency::Hyperv,
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

/// The install on macOS: isothrum-machine's Homebrew formulae, vendors' installers and
/// HashiCorp's signed package.
fn macos_plan(dep: Dependency, host: Host) -> Result<Plan> {
    isothrum_machine::install::macos::install(dep.machine(), host.packages.map(Packages::isothrum), host.arch.isothrum()).map_err(refusal)
}

/// The install on Windows: isothrum-machine's winget packages and elevated installers.
fn windows_plan(dep: Dependency, arch: Arch) -> Result<Plan> {
    isothrum_machine::install::windows::install(dep.machine(), arch.isothrum()).map_err(refusal)
}

/// How root steps run on this Linux machine (isothrum-machine's [`linux::Root`]): through pkexec's
/// prompt, or unattended (see `platform::elevation`) as root or through `sudo -n`; with this
/// process's proxy, which a root step reads from a private file.
pub(super) fn linux_root() -> linux::Root {
    let elevation =
        if crate::platform::elevation::unattended() { Elevation::Unattended { root: crate::platform::elevation::is_root() } } else { Elevation::Prompt };
    linux::Root { elevation, proxy: linux::proxy_assignments(|name| std::env::var(name).ok()) }
}

/// Why isothrum-machine can't make a plan here, in the player's words.
pub(super) fn refusal(refused: Refusal) -> Error {
    match refused {
        Refusal::NotHere { dependency, os } => not_here(Dependency::of_machine(dependency), Os::of_isothrum(os)),
        Refusal::WithoutPackages { dependency } => Error::Invalid(format!(
            "This distribution isn't supported by the guided setup yet (it has neither apt nor dnf): install {} yourself, then re-check.",
            Dependency::of_machine(dependency).name_on(Os::Linux)
        )),
        Refusal::AptOnly { dependency } => Error::Invalid(format!(
            "The guided setup installs {} through apt only for now: install it yourself, then re-check.",
            Dependency::of_machine(dependency).name_on(Os::Linux)
        )),
        Refusal::NoHomebrew => Error::Invalid("Homebrew is required. Install it from https://brew.sh, then try again.".into()),
        Refusal::HomebrewNeeded => Error::Invalid("Homebrew is needed to remove this tool.".into()),
        Refusal::NoVirtualboxImage => Error::Invalid("VirtualBox's installer image is needed to run its uninstaller.".into()),
        Refusal::NotOnWindowsOnArm { provider, .. } => Error::Invalid(format!(
            "{} doesn't run on Windows on Arm. Run VM labs with Hyper-V, on a server or in the cloud.",
            Provider::from(provider).label_on(Os::Windows)
        )),
        Refusal::WindowsFeature { dependency: isothrum_machine::install::Dependency::Wsl } => {
            Error::Invalid("Turn WSL off in Windows Features if you no longer need it.".into())
        }
        Refusal::WindowsFeature { .. } => Error::Invalid("Turn Hyper-V off in Windows Features if you no longer need it.".into()),
        other => Error::Io(std::io::Error::other(other.to_string())),
    }
}

/// The install on Linux: isothrum-machine's root script, for the player `user` (Linux joins them
/// to the groups a tool's devices need).
fn linux_plan(dep: Dependency, packages: Option<Packages>, user: Option<&str>) -> Result<Plan> {
    linux::install(dep.machine(), packages.map(Packages::isothrum), user, &linux_root()).map_err(refusal)
}

/// The ordered install steps for a dependency on `host`, for the player `user` (Linux joins
/// them to the groups a tool's devices need).
pub(super) fn plan_on(dep: Dependency, host: Host, user: Option<&str>) -> Result<Plan> {
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
fn plan(dep: Dependency) -> Result<Plan> {
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
    run_plan(&steps, &mut on_line).await?;
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
    let step = isothrum_machine::install::step("vagrant", &["plugin", "install", &plugin]);
    on_line(isothrum_machine::install::command_line(&step));
    run_step(&step, &mut on_line).await?;
    on_line("Done. Re-checking this machine…".into());
    Ok(())
}

#[cfg(test)]
mod tests;
