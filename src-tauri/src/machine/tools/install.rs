//! Assisted, one-click install of lab dependencies, streaming the package manager's output.
//! It never shows a custom password prompt: it uses the OS's own trusted flow (winget's
//! UAC, Linux pkexec, or downloading the official app/installer for macOS casks and
//! letting the tool's own installer run).

use serde::Deserialize;

#[cfg(target_os = "macos")]
use super::scripts::fetch_and_open;
#[cfg(target_os = "linux")]
use super::scripts::{
    awscli_script, azurecli_script, docker_script, gcloud_script, libvirt_script, terraform_script, vagrant_script, virtualbox_script, vmware_utility_script,
};
#[cfg(any(target_os = "macos", windows))]
use super::steps::step;
use super::steps::{Step, command_line, run_step, run_steps};
use super::uninstall;
use crate::error::{Error, Result};
use crate::platform::policy::{self, Key};
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
            },
            Step {
                program: brew.to_string(),
                args: vec!["install".into(), "--cask".into(), "azure-cli-preview".into()],
                note: Some("Installing the prebuilt Azure CLI cask (avoids a long source build on this macOS).".into()),
            },
        ],
        Dependency::Gcloud => vec![step(brew, &["install", "--cask", "google-cloud-sdk"])],
        Dependency::VmwareUtility => vec![super::vmware_utility::mac_install(std::env::consts::ARCH)],
    })
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
        (_, Some(id)) => Ok(vec![step("winget", &["install", "-e", "--id", id, "--accept-source-agreements", "--accept-package-agreements"])]),
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
/// the script through `env`: behind a corporate proxy apt and curl need them.
#[cfg(any(target_os = "linux", test))]
fn as_root(script: String, proxy: Vec<String>) -> Step {
    let mut args = vec!["env".to_string()];
    args.extend(proxy);
    args.extend(["sh".to_string(), "-c".to_string(), script]);
    Step { program: "pkexec".into(), args, note: None }
}

/// `script` run as root through pkexec ([`as_root`]), with this process's proxy: every install
/// and removal step on Linux.
#[cfg(target_os = "linux")]
pub(super) fn root(script: String) -> Step {
    as_root(script, crate::platform::proxy::assignments(|name| std::env::var(name).ok()))
}

/// The install steps on Linux: apt and the vendors' apt repositories, as root through pkexec.
#[cfg(target_os = "linux")]
fn linux_plan(dep: Dependency) -> Result<Vec<Step>> {
    linux_plan_with(dep, apt_here())
}

/// [`linux_plan`] on a machine with apt (`apt`) or without.
#[cfg(target_os = "linux")]
fn linux_plan_with(dep: Dependency, apt: bool) -> Result<Vec<Step>> {
    Ok(match dep {
        Dependency::Wsl | Dependency::Hyperv | Dependency::Utm => return Err(not_here(dep)),
        _ if !apt => return Err(without_apt(dep)),
        Dependency::Docker => vec![root(docker_script(crate::platform::exec::login_name().as_deref()))],
        // From HashiCorp's apt repository, not the distribution's (older than labs need).
        Dependency::Vagrant => vec![root(vagrant_script())],
        // Terraform via HashiCorp's official apt repo (best effort across Debian/Ubuntu).
        Dependency::Terraform => vec![root(terraform_script())],
        Dependency::Virtualbox => vec![root(virtualbox_script())],
        Dependency::Qemu => vec![root("apt-get update && apt-get install -y qemu-system qemu-utils".into())],
        Dependency::Libvirt => vec![root(libvirt_script(crate::platform::exec::login_name().as_deref()))],
        // Cloud CLIs: AWS CLI v2 from Amazon's signed bundle, in a private temporary directory;
        // Azure CLI and gcloud from Microsoft's and Google's apt repositories, their keys pinned.
        Dependency::Awscli => vec![root(awscli_script())],
        Dependency::Azurecli => vec![root(azurecli_script())],
        Dependency::Gcloud => vec![root(gcloud_script())],
        Dependency::VmwareUtility => vec![root(vmware_utility_script())],
    })
}

/// Whether apt is on this machine: every Linux install goes through it (and the vendors' apt
/// repositories), so a distribution without it (Fedora, openSUSE, Arch) can't install from here yet.
#[cfg(target_os = "linux")]
fn apt_here() -> bool {
    std::env::var_os("PATH").is_some_and(|path| std::env::split_paths(&path).any(|dir| dir.join("apt-get").is_file()))
}

/// What an install of `dep` says on a Linux distribution without apt: the guided setup doesn't
/// install there yet, so the player installs it themselves; instead of an apt command that fails.
#[cfg(any(target_os = "linux", test))]
pub(crate) fn without_apt(dep: Dependency) -> Error {
    Error::Invalid(format!("This distribution isn't supported by the guided setup yet (it has no apt): install {} yourself, then re-check.", dep.name()))
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
    let os = match std::env::consts::OS {
        "macos" => "macOS",
        "windows" => "Windows",
        _ => "Linux",
    };
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
    let step = Step { program: "vagrant".into(), args: vec!["plugin".into(), "install".into(), plugin], note: None };
    on_line(command_line(&step));
    run_step(&step, &mut on_line).await?;
    on_line("Done. Re-checking this machine…".into());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn docker_is_named_as_the_setup_installs_it_on_each_os() {
        assert_eq!(Dependency::Docker.name_on("linux"), "Docker Engine");
        assert_eq!(Dependency::Docker.name_on("macos"), "Docker Desktop");
        assert_eq!(Dependency::Docker.name_on("windows"), "Docker Desktop");
        assert_eq!(Dependency::Awscli.name_on("linux"), "AWS CLI");
    }

    #[test]
    fn each_tool_runs_on_its_os_and_is_refused_elsewhere_in_one_message() {
        for (dep, oses) in [
            (Dependency::Utm, ["macos"].as_slice()),
            (Dependency::Libvirt, &["linux"]),
            (Dependency::Wsl, &["windows"]),
            (Dependency::Hyperv, &["windows"]),
            (Dependency::Docker, &["macos", "linux", "windows"]),
            (Dependency::Qemu, &["macos", "linux", "windows"]),
        ] {
            for os in ["macos", "linux", "windows"] {
                assert_eq!(dep.runs_on(os), oses.contains(&os), "{dep:?} on {os}");
            }
        }
        // Here: every tool this OS doesn't have is refused before any step, saying so.
        for dep in Dependency::ALL.into_iter().filter(|d| !d.runs_on(std::env::consts::OS)) {
            let said = plan(dep).err().map(|e| e.to_string()).unwrap_or_default();
            assert_eq!(said, super::not_here(dep).to_string());
            assert!(said.starts_with(&format!("{} doesn't run on ", dep.name())), "{said}");
        }
    }

    #[tokio::test]
    async fn the_policy_refuses_every_one_click_install_before_anything_runs() {
        let _policy = policy::Override::with(&[Key::DisableToolInstalls]);
        let tools = crate::platform::exec::fake::Tools::rules(&[]);
        let t = crate::testing::test_app::TestApp::new();
        let mut lines = Vec::new();
        let err = install_dependency(t.dirs(), Dependency::Docker, |l| lines.push(l)).await.unwrap_err();
        assert_eq!(err.code(), crate::error::Code::TurnedOffByPolicy { setting: "DisableToolInstalls".into() });
        assert_eq!(lines, [err.to_string()], "said on the install's log");
        let err = install_vagrant_plugin("vagrant-vmware-esxi".into(), |_| {}).await.unwrap_err();
        assert!(err.to_string().contains("DisableToolInstalls"), "{err}");
        assert!(tools.calls().is_empty(), "{:?}", tools.calls());
    }

    #[test]
    fn a_root_install_carries_the_proxy_through_pkexec() {
        let step = as_root("apt-get update".into(), vec!["https_proxy=http://proxy.corp:3128".into(), "no_proxy=.corp".into()]);
        assert_eq!(step.program, "pkexec");
        assert_eq!(step.args, ["env", "https_proxy=http://proxy.corp:3128", "no_proxy=.corp", "sh", "-c", "apt-get update"]);
        assert_eq!(as_root("true".into(), Vec::new()).args, ["env", "sh", "-c", "true"]);
    }

    #[test]
    fn only_vagrant_plugin_names_are_installed() {
        assert!(valid_plugin("vagrant-vmware-esxi") && valid_plugin("vagrant_proxmox"));
        assert!(!valid_plugin("vagrant-x; touch /tmp/x") && !valid_plugin("--plugin-source") && !valid_plugin("rails"));
        assert!(!valid_plugin(&format!("vagrant-{}", "a".repeat(64))));
    }

    /// The plan for every dependency on this OS: the ones that install here have steps, the
    /// others say why not.
    fn plans() -> Vec<(Dependency, Result<Vec<Step>>)> {
        Dependency::ALL.into_iter().map(|d| (d, plan(d))).collect()
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_installs_through_homebrew() {
        if crate::platform::brew_bin().is_none() {
            assert!(plan(Dependency::Docker).err().unwrap().to_string().contains("Homebrew is required"));
            return;
        }
        for (dep, p) in plans() {
            match dep {
                Dependency::Libvirt | Dependency::Wsl | Dependency::Hyperv => assert!(p.is_err(), "{dep:?}"),
                _ => assert!(!p.unwrap().is_empty(), "{dep:?}"),
            }
        }
        let azure = plan(Dependency::Azurecli).unwrap();
        assert_eq!(azure.len(), 3);
        assert!(azure[2].note.is_some() && azure[2].args.contains(&"azure-cli-preview".to_string()));
        // QEMU is one shell step: a best-effort Homebrew update that never aborts the install
        // (`|| …`), then the install itself. It must not be a second, fatal `brew update` step.
        let qemu = plan(Dependency::Qemu).unwrap();
        assert_eq!(qemu.len(), 1);
        assert_eq!(qemu[0].program, "sh");
        let script = qemu[0].args.join(" ");
        assert!(script.contains("update ||") && script.contains("install qemu"), "{script}");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_installs_through_pkexec() {
        for (dep, p) in plans() {
            match dep {
                Dependency::Utm | Dependency::Wsl | Dependency::Hyperv => assert!(p.is_err(), "{dep:?}"),
                _ => assert_eq!(p.unwrap()[0].program, "pkexec", "{dep:?}"),
            }
        }
        // libvirt brings what the vagrant-libvirt plugin builds against, and joins its group.
        let libvirt = plan(Dependency::Libvirt).unwrap()[0].args.join(" ");
        assert!(libvirt.contains("libvirt-dev") && libvirt.contains("build-essential"), "{libvirt}");
        let aws = plan(Dependency::Awscli).unwrap()[0].args.join(" ");
        assert!(aws.contains("awscli-exe-linux") && !aws.contains("/tmp/aws"), "{aws}");
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_installs_through_winget_and_wsl_through_its_installer() {
        for (dep, p) in plans() {
            match dep {
                Dependency::Utm | Dependency::Libvirt => assert!(p.is_err(), "{dep:?}"),
                Dependency::Virtualbox | Dependency::VmwareUtility if std::env::consts::ARCH == "aarch64" => assert!(p.is_err()),
                Dependency::Wsl | Dependency::VmwareUtility | Dependency::Hyperv => assert_eq!(p.unwrap()[0].program, "powershell"),
                _ => {
                    let steps = p.unwrap();
                    assert_eq!(steps[0].program, "winget");
                    assert!(steps[0].args.contains(&"--accept-package-agreements".to_string()));
                }
            }
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn vagrant_comes_from_hashicorps_repository_not_the_distributions() {
        // Regression: the setup ran `apt-get install vagrant`, Debian 13's 2.3.8, and every VM lab
        // then refused it ("needs Vagrant 2.4.0 or later").
        let plan = linux_plan_with(Dependency::Vagrant, true).unwrap();
        let script = plan.iter().flat_map(|s| s.args.iter()).cloned().collect::<Vec<_>>().join(" ");
        assert!(script.contains("apt.releases.hashicorp.com") && script.contains("apt-get install -y vagrant"), "{script}");
        assert!(!script.contains("apt-get update && apt-get install -y vagrant"), "the distribution's package: {script}");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_distribution_without_apt_is_told_to_install_it_itself_before_anything_runs() {
        // Regression: Fedora, openSUSE and Arch got `apt-get: not found` from a pkexec prompt.
        for dep in [Dependency::Vagrant, Dependency::Virtualbox, Dependency::Libvirt, Dependency::Qemu, Dependency::Docker] {
            let err = linux_plan_with(dep, false).err().unwrap_or_else(|| panic!("{dep:?} refused"));
            assert_eq!(err.to_string(), without_apt(dep).to_string());
            assert!(linux_plan_with(dep, true).is_ok(), "{dep:?}");
        }
        assert!(linux_plan_with(Dependency::Utm, false).err().is_some_and(|e| e.to_string().starts_with("UTM doesn't run on Linux")));
    }

    #[test]
    fn without_apt_the_install_says_the_distribution_isnt_supported_yet_and_what_to_install() {
        let err = without_apt(Dependency::Virtualbox).to_string();
        assert_eq!(err, "This distribution isn't supported by the guided setup yet (it has no apt): install VirtualBox yourself, then re-check.");
    }

    #[test]
    fn hyper_v_is_turned_on_by_dism_elevated_and_refused_on_windows_home() {
        // DISM's "done, restart to finish" (3010) is a success; Home has no Hyper-V to turn on.
        assert!(HYPERV_ON.contains("/FeatureName:Microsoft-Hyper-V") && HYPERV_ON.contains("/NoRestart") && HYPERV_ON.contains("-Verb RunAs"));
        assert!(HYPERV_ON.contains("-eq 3010) { exit 0 }") && HYPERV_ON.contains("EditionID -like 'Core*'"));
    }

    #[test]
    fn dependencies_use_lowercase_names() {
        assert_eq!(serde_json::to_string(&Dependency::Azurecli).unwrap(), "\"azurecli\"");
        assert_eq!(serde_json::from_str::<Dependency>("\"wsl\"").unwrap(), Dependency::Wsl);
        assert_eq!(serde_json::to_string(&Dependency::Hyperv).unwrap(), "\"hyperv\"");
    }

    use std::cell::Cell;
    use std::rc::Rc;

    use crate::platform::exec::fake::{Tools, fail, ok};
    use crate::testing::test_app::{Sink, TestApp};

    /// A dependency this OS doesn't install (on macOS without Homebrew, none does).
    fn not_here() -> Dependency {
        if cfg!(windows) { Dependency::Utm } else { Dependency::Wsl }
    }

    /// Scripted tools for an install of Terraform: `terraform version` answers once `installed`
    /// is set, and the install step (whichever package manager runs it) sets it when `works`.
    fn terraform_tools(installed: Rc<Cell<bool>>, works: bool) -> Tools {
        Tools::new(move |program, _| {
            if program == "terraform" {
                return if installed.get() { ok("Terraform v1.9.8") } else { fail("terraform: not found") };
            }
            if !works {
                return fail("E: Unable to locate package terraform");
            }
            installed.set(true);
            ok("Setting up terraform (1.9.8-1) ...")
        })
    }

    #[tokio::test]
    async fn a_dependency_this_os_does_not_install_is_refused_in_the_log_before_running_anything() {
        let t = TestApp::new();
        let tools = Tools::rules(&[]);
        let log = Sink::default();
        let err = install_dependency(t.dirs(), not_here(), log.on_line()).await.unwrap_err();
        assert_eq!(log.lines(), [err.to_string()]);
        assert!(tools.calls().is_empty(), "{:?}", tools.calls());
        assert!(uninstall::installed_tools(t.dirs()).await.is_empty());
    }

    #[tokio::test]
    async fn an_install_streams_its_steps_and_records_a_tool_it_added() {
        let Ok(steps) = plan(Dependency::Terraform) else { return }; // macOS without Homebrew
        let t = TestApp::new();
        let tools = terraform_tools(Rc::new(Cell::new(false)), true);
        let log = Sink::default();
        install_dependency(t.dirs(), Dependency::Terraform, log.on_line()).await.unwrap();
        let lines = log.lines();
        assert_eq!(lines.first(), Some(&command_line(&steps[0])), "{lines:?}");
        assert!(lines.contains(&"Setting up terraform (1.9.8-1) ...".to_string()), "{lines:?}");
        assert_eq!(lines.last().map(String::as_str), Some("Done. Re-checking this machine…"));
        assert!(tools.called(&format!("{} {}", steps[0].program, steps[0].args.join(" "))));
        let ours: Vec<_> = uninstall::installed_tools(t.dirs()).await.into_iter().map(|t| t.dependency).collect();
        assert_eq!(ours, [Dependency::Terraform]);

        // Cleanup removes it, then forgets it: a second removal is refused.
        let log = Sink::default();
        uninstall::uninstall_dependency(t.dirs(), Dependency::Terraform, log.on_line()).await.unwrap();
        assert_eq!(log.lines().last().map(String::as_str), Some("Removed."));
        assert!(uninstall::installed_tools(t.dirs()).await.is_empty());
        let err = uninstall::uninstall_dependency(t.dirs(), Dependency::Terraform, Sink::default().on_line()).await.unwrap_err();
        assert_eq!(err.to_string(), "Cyber CTF didn't install this tool, so it won't remove it.");
    }

    #[tokio::test]
    async fn a_tool_that_was_already_there_is_never_recorded() {
        if plan(Dependency::Terraform).is_err() {
            return;
        }
        let t = TestApp::new();
        let _tools = terraform_tools(Rc::new(Cell::new(true)), true);
        install_dependency(t.dirs(), Dependency::Terraform, Sink::default().on_line()).await.unwrap();
        assert!(uninstall::installed_tools(t.dirs()).await.is_empty());
    }

    #[tokio::test]
    async fn a_tool_not_found_yet_counts_once_a_later_check_finds_it() {
        if plan(Dependency::Terraform).is_err() {
            return;
        }
        let t = TestApp::new();
        let present = Rc::new(Cell::new(false));
        // The step "works" but the tool isn't on PATH yet (an installer left open).
        let probe = present.clone();
        let _tools = Tools::new(move |program, _| if program == "terraform" && !probe.get() { fail("not found") } else { ok("") });
        install_dependency(t.dirs(), Dependency::Terraform, Sink::default().on_line()).await.unwrap();
        assert!(uninstall::installed_tools(t.dirs()).await.is_empty(), "pending, not shown");
        present.set(true);
        let ours: Vec<_> = uninstall::installed_tools(t.dirs()).await.into_iter().map(|t| t.dependency).collect();
        assert_eq!(ours, [Dependency::Terraform]);
    }

    #[tokio::test]
    async fn a_failed_install_says_so_and_records_nothing() {
        if plan(Dependency::Terraform).is_err() {
            return;
        }
        let t = TestApp::new();
        let _tools = terraform_tools(Rc::new(Cell::new(false)), false);
        let log = Sink::default();
        let err = install_dependency(t.dirs(), Dependency::Terraform, log.on_line()).await.unwrap_err();
        assert!(err.to_string().contains("exited with"), "{err}");
        assert!(log.lines().contains(&"E: Unable to locate package terraform".to_string()));
        assert!(!log.lines().iter().any(|l| l.starts_with("Done.")));
        assert!(uninstall::installed_tools(t.dirs()).await.is_empty());
    }

    #[tokio::test]
    async fn an_invalid_plugin_is_refused_before_running_anything() {
        let err = install_vagrant_plugin("rails".into(), |_| {}).await.unwrap_err();
        assert_eq!(err.to_string(), "invalid plugin name `rails`");
    }
}
