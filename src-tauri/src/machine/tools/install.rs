//! Assisted, one-click install of lab dependencies, streaming the package manager's output.
//! It never shows a custom password prompt: it uses the OS's own trusted flow (winget's
//! UAC, Linux pkexec, or downloading the official app/installer for macOS casks and
//! letting the tool's own installer run).
//!
//! The tools, their rows (where each exists, what proves it installed, where else the player gets
//! it) and their install and removal plans are isothrum-machine's ([`Dependency`]); the launcher
//! names each tool ([`name`]), says why a plan can't be made in the player's words ([`refusal`])
//! and records what it installed. Every plan is built on any OS, so the tests read every row.

use super::steps::{Plan, run_plan, run_step};
use super::uninstall;
use crate::error::{Error, Result};
use crate::platform::policy::{self, Key};
use crate::runtime::providers::Provider;
use isothrum_machine::install::{Elevation, Refusal, linux};
use isothrum_machine::{Arch, Host, HostOs, Packages};

/// A tool the guided setup installs, as the webview names it: isothrum-machine's, whose rows say
/// where each runs and what proves it installed.
pub use isothrum_machine::install::Dependency;

/// `dep`'s name on this OS, as the setup installs it here (Docker Engine on Linux, from Docker's
/// repository or the distribution's `docker.io`; Docker Desktop elsewhere): what a message and
/// Settings' list of installed tools call it.
pub fn name(dep: Dependency) -> &'static str {
    name_on(dep, isothrum_runner::this_host())
}

/// [`name`] on `os`.
pub(crate) fn name_on(dep: Dependency, os: HostOs) -> &'static str {
    match dep {
        Dependency::Docker if os == HostOs::Linux => "Docker Engine",
        Dependency::Docker => "Docker Desktop",
        Dependency::Vagrant => "Vagrant",
        Dependency::Terraform => "Terraform",
        Dependency::Awscli => "AWS CLI",
        Dependency::Azurecli => "Azure CLI",
        Dependency::Gcloud => "Google Cloud CLI",
        Dependency::Wsl => "WSL 2",
        Dependency::VmwareUtility => "Vagrant VMware Utility",
        Dependency::Virtualbox | Dependency::Qemu | Dependency::Utm | Dependency::Libvirt | Dependency::Hyperv => {
            dep.hypervisor().map_or("", |p| Provider::from(p).label())
        }
    }
}

/// Whether `dep` exists on `os`. An install or a removal elsewhere is refused ([`not_here`]).
pub fn runs_on(dep: Dependency, os: HostOs) -> bool {
    dep.runs_on(os)
}

/// The install on macOS: isothrum-machine's Homebrew formulae, vendors' installers and
/// HashiCorp's signed package.
fn macos_plan(dep: Dependency, host: &Host) -> Result<Plan> {
    isothrum_machine::install::macos::install(dep, host.packages, host.arch).map_err(refusal)
}

/// The install on Windows: isothrum-machine's winget packages and elevated installers.
fn windows_plan(dep: Dependency, arch: Arch) -> Result<Plan> {
    isothrum_machine::install::windows::install(dep, arch).map_err(refusal)
}

/// How root steps run on this Linux machine: through pkexec's prompt, or unattended (see
/// `platform::elevation`) as root or through `sudo -n`.
pub(crate) fn elevation() -> Elevation {
    if crate::platform::elevation::unattended() { Elevation::Unattended { root: crate::platform::elevation::is_root() } } else { Elevation::Prompt }
}

/// How root steps run on this Linux machine (isothrum-machine's [`linux::Root`]): [`elevation`]'s
/// way, with this process's proxy, which a root step reads from a private file.
pub(super) fn linux_root() -> linux::Root {
    linux::Root { elevation: elevation(), proxy: linux::proxy_assignments(|name| std::env::var(name).ok()) }
}

/// Why isothrum-machine can't make a plan here, in the player's words.
pub(super) fn refusal(refused: Refusal) -> Error {
    match refused {
        Refusal::NotHere { dependency, os } => not_here(dependency, os),
        Refusal::WithoutPackages { dependency } => Error::Invalid(format!(
            "This distribution isn't supported by the guided setup yet (it has neither apt nor dnf): install {} yourself, then re-check.",
            name_on(dependency, HostOs::Linux)
        )),
        Refusal::AptOnly { dependency } => Error::Invalid(format!(
            "The guided setup installs {} through apt only for now: install it yourself, then re-check.",
            name_on(dependency, HostOs::Linux)
        )),
        Refusal::NoHomebrew => Error::Invalid("Homebrew is required. Install it from https://brew.sh, then try again.".into()),
        Refusal::HomebrewNeeded => Error::Invalid("Homebrew is needed to remove this tool.".into()),
        Refusal::NoVirtualboxImage => Error::Invalid("VirtualBox's installer image is needed to run its uninstaller.".into()),
        Refusal::NotOnWindowsOnArm { provider, .. } => Error::Invalid(format!(
            "{} doesn't run on Windows on Arm. Run VM labs with Hyper-V, on a server or in the cloud.",
            Provider::from(provider).label_on(HostOs::Windows)
        )),
        Refusal::WindowsFeature { dependency: Dependency::Wsl } => Error::Invalid("Turn WSL off in Windows Features if you no longer need it.".into()),
        Refusal::WindowsFeature { .. } => Error::Invalid("Turn Hyper-V off in Windows Features if you no longer need it.".into()),
        other => Error::Io(std::io::Error::other(other.to_string())),
    }
}

/// The install on Linux: isothrum-machine's root script, for the player `user` (Linux joins them
/// to the groups a tool's devices need).
fn linux_plan(dep: Dependency, packages: Option<Packages>, user: Option<&str>) -> Result<Plan> {
    linux::install(dep, packages, user, &linux_root()).map_err(refusal)
}

/// The ordered install steps for a dependency on `host`, for the player `user` (Linux joins
/// them to the groups a tool's devices need).
pub(super) fn plan_on(dep: Dependency, host: &Host, user: Option<&str>) -> Result<Plan> {
    if !runs_on(dep, host.os) {
        return Err(not_here(dep, host.os));
    }
    match host.os {
        HostOs::Macos => macos_plan(dep, host),
        HostOs::Windows => windows_plan(dep, host.arch),
        HostOs::Linux => linux_plan(dep, host.packages, user),
    }
}

/// The ordered install steps for a dependency on this computer. One that doesn't run here is
/// refused before the package manager is looked up.
async fn plan(dep: Dependency) -> Result<Plan> {
    let os = isothrum_runner::this_host();
    if !runs_on(dep, os) {
        return Err(not_here(dep, os));
    }
    plan_on(dep, &crate::runtime::providers::host().await, crate::platform::exec::login_name().as_deref())
}

/// Why `dep` can't be installed or removed on `os`: it doesn't run there ([`Dependency::runs_on`]).
pub(super) fn not_here(dep: Dependency, os: HostOs) -> Error {
    Error::Invalid(format!("{} doesn't run on {}: there is nothing of it to install or remove here.", name_on(dep, os), isothrum_machine::host::os_name(os)))
}

/// Refused when this computer's policy turns one-click installs off (`DisableToolInstalls`): the
/// organization installs its tools itself. Said on the install's log too.
pub(super) fn installs_allowed(on_line: &mut impl FnMut(String)) -> Result<()> {
    policy::current().check(Key::DisableToolInstalls).inspect_err(|e| on_line(e.to_string()))
}

/// Installs a dependency, streaming each command and its output to the UI.
pub async fn install_dependency(dirs: crate::platform::AppDirs, dependency: Dependency, mut on_line: impl FnMut(String)) -> Result<()> {
    installs_allowed(&mut on_line)?;
    let steps = plan(dependency).await.inspect_err(|e| on_line(e.to_string()))?;
    let before = uninstall::present(dependency).await;
    run_plan(&steps, &mut on_line).await?;
    // Windows: the installer wrote its folder to the registry PATH; pick it up before looking.
    crate::platform::env_path::refresh().await;
    // Remembered so Cleanup can offer to remove what Cyber CTF installed, and only that: a tool
    // that was already there is never recorded, and one whose installer was only opened (macOS
    // casks) or isn't found yet counts once a later check finds it.
    if let Some(before) = before {
        let after = uninstall::present(dependency).await.unwrap_or(false);
        uninstall::record_outcome(&dirs, dependency, isothrum_machine::install::record::outcome(before, after));
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

/// Installs a Vagrant plugin (userland, no admin), streaming the output. isothrum-machine refuses
/// a name that isn't a `vagrant-*` / `vagrant_*` identifier.
pub async fn install_vagrant_plugin(plugin: String, mut on_line: impl FnMut(String)) -> Result<()> {
    installs_allowed(&mut on_line)?;
    let Some(step) = isothrum_machine::install::vagrant_plugin(&plugin) else {
        return Err(Error::Invalid(format!("invalid plugin name `{plugin}`")));
    };
    on_line(isothrum_machine::install::command_line(&step));
    run_step(&step, &mut on_line).await?;
    on_line("Done. Re-checking this machine…".into());
    Ok(())
}

#[cfg(test)]
mod tests;
