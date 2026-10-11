//! Assisted, one-click install of lab dependencies, streaming the package manager's output.
//! It never shows a custom password prompt: it uses the OS's own trusted flow (winget's
//! UAC, Linux pkexec, or downloading the official app/installer for macOS casks and
//! letting the tool's own installer run).

use serde::Deserialize;
use tauri::ipc::Channel;

#[cfg(target_os = "macos")]
use super::scripts::fetch_and_open;
#[cfg(target_os = "linux")]
use super::scripts::{TERRAFORM_SCRIPT, VIRTUALBOX_SCRIPT, awscli_script, azurecli_script, docker_script};
use super::steps::{Step, channel_log, command_line, run_step, run_steps, step};
use super::uninstall;
use crate::error::{Error, Result};

#[derive(Deserialize, serde::Serialize, Clone, Copy, PartialEq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Dependency {
    Docker,
    Vagrant,
    Terraform,
    Virtualbox,
    Qemu,
    Utm,
    Libvirt,
    Awscli,
    Azurecli,
    Gcloud,
    /// Windows Subsystem for Linux (WSL 2), enabled through its own installer.
    Wsl,
}

/// The ordered install steps for a dependency on this OS.
fn plan(dep: Dependency) -> Result<Vec<Step>> {
    #[cfg(target_os = "macos")]
    {
        let brew = super::brew_bin().ok_or_else(|| Error::Invalid("Homebrew is required. Install it from https://brew.sh, then try again.".into()))?;
        Ok(match dep {
            // Docker Desktop: download the app with brew, then open its installer; the user
            // finishes in Docker's own flow (includes the docker CLI + compose).
            Dependency::Docker => vec![fetch_and_open(&brew, "docker-desktop", "Docker Desktop")],
            // Admin-requiring: download with brew, then open the tool's native installer.
            Dependency::Vagrant => vec![fetch_and_open(&brew, "vagrant", "Vagrant")],
            // Terraform is a brew formula (no admin); full tap name since core dropped it.
            Dependency::Terraform => vec![step(brew.clone(), &["install", "hashicorp/tap/terraform"])],
            Dependency::Virtualbox => vec![fetch_and_open(&brew, "virtualbox", "VirtualBox")],
            // QEMU is a brew formula (no admin); UTM is a cask with its own installer.
            Dependency::Qemu => vec![step(brew.clone(), &["install", "qemu"])],
            Dependency::Utm => vec![fetch_and_open(&brew, "utm", "UTM")],
            Dependency::Libvirt => return Err(Error::Invalid("libvirt isn't used on macOS; use QEMU or UTM instead.".into())),
            Dependency::Wsl => return Err(Error::Invalid("WSL is only available on Windows.".into())),
            // Cloud CLIs (brew formulae; gcloud is a cask).
            Dependency::Awscli => vec![step(brew.clone(), &["install", "awscli"])],
            // Microsoft's prebuilt cask instead of the `azure-cli` formula: on recent macOS the
            // formula has no bottle and builds its whole tree (llvm, rust) from source, which takes
            // ~an hour. The cask is a signed download. `trust` is needed because the tap is third-party.
            // https://learn.microsoft.com/cli/azure/install-azure-cli-macos
            Dependency::Azurecli => vec![
                step(brew.clone(), &["tap", "azure/azure-cli"]),
                step(brew.clone(), &["trust", "azure/azure-cli"]),
                Step {
                    program: brew.clone(),
                    args: vec!["install".into(), "--cask".into(), "azure-cli-preview".into()],
                    note: Some("Installing the prebuilt Azure CLI cask (avoids a long source build on this macOS).".into()),
                },
            ],
            Dependency::Gcloud => vec![step(brew.clone(), &["install", "--cask", "google-cloud-sdk"])],
        })
    }
    #[cfg(target_os = "windows")]
    {
        let id = match dep {
            Dependency::Docker => "Docker.DockerDesktop",
            Dependency::Vagrant => "Hashicorp.Vagrant",
            Dependency::Terraform => "Hashicorp.Terraform",
            Dependency::Virtualbox if std::env::consts::ARCH == "aarch64" => {
                return Err(Error::Invalid("VirtualBox doesn't run on Windows on Arm. Run VM labs with Hyper-V, on a server or in the cloud.".into()));
            }
            Dependency::Virtualbox => "Oracle.VirtualBox",
            Dependency::Qemu => "SoftwareFreedomConservancy.QEMU",
            Dependency::Utm => return Err(Error::Invalid("UTM is only available on macOS.".into())),
            Dependency::Libvirt => return Err(Error::Invalid("libvirt is Linux-only; on Windows use Hyper-V or WSL.".into())),
            Dependency::Awscli => "Amazon.AWSCLI",
            Dependency::Azurecli => "Microsoft.AzureCLI",
            Dependency::Gcloud => "Google.CloudSDK",
            // Turns on the WSL and Virtual Machine Platform features and installs the WSL 2 kernel.
            // Elevated through Windows' own UAC prompt; the features need a restart to take effect.
            Dependency::Wsl => {
                return Ok(vec![step(
                    "powershell",
                    &["-NoProfile", "-Command", "Start-Process -FilePath wsl.exe -ArgumentList '--install','--no-distribution' -Verb RunAs -Wait"],
                )]);
            }
        };
        Ok(vec![step("winget", &["install", "-e", "--id", id, "--accept-source-agreements", "--accept-package-agreements"])])
    }
    #[cfg(target_os = "linux")]
    {
        // pkexec raises a graphical password prompt (polkit) for the privileged install.
        let root = |script: String| Step { program: "pkexec".into(), args: vec!["sh".into(), "-c".into(), script], note: None };
        Ok(match dep {
            Dependency::Wsl => return Err(Error::Invalid("WSL is only available on Windows.".into())),
            Dependency::Docker => vec![root(docker_script(crate::exec::login_name().as_deref()))],
            Dependency::Vagrant => vec![step("pkexec", &["sh", "-c", "apt-get update && apt-get install -y vagrant"])],
            // Terraform via HashiCorp's official apt repo (best effort across Debian/Ubuntu).
            Dependency::Terraform => vec![root(TERRAFORM_SCRIPT.into())],
            Dependency::Virtualbox => vec![root(VIRTUALBOX_SCRIPT.into())],
            Dependency::Qemu => vec![step("pkexec", &["sh", "-c", "apt-get update && apt-get install -y qemu-system qemu-utils"])],
            Dependency::Libvirt => vec![step("pkexec", &["sh", "-c", "apt-get update && apt-get install -y libvirt-daemon-system virt-manager"])],
            Dependency::Utm => return Err(Error::Invalid("UTM is only available on macOS.".into())),
            // Cloud CLIs via each vendor's official installer (best effort across distros).
            // AWS CLI v2 from the official bundle, in a private temporary directory.
            Dependency::Awscli => vec![root(awscli_script())],
            Dependency::Azurecli => vec![root(azurecli_script())],
            // The SDK installs per user, without root: under pkexec it landed in /root, which the
            // player can't read. It goes to ~/google-cloud-sdk, whose bin the app adds to PATH.
            Dependency::Gcloud => vec![step("sh", &["-c", "curl -sSL https://sdk.cloud.google.com | bash -s -- --disable-prompts --install-dir=\"$HOME\""])],
        })
    }
}

/// Installs a dependency, streaming each command and its output to the UI.
#[tauri::command]
pub async fn install_dependency(app: tauri::AppHandle, dependency: Dependency, logs: Channel<String>) -> Result<()> {
    let mut on_line = channel_log(logs);
    let steps = plan(dependency).inspect_err(|e| on_line(e.to_string()))?;
    let before = uninstall::present(dependency).await;
    run_steps(&steps, &mut on_line).await?;
    // Remembered so Cleanup can offer to remove what Cyber CTF installed, and only that: a tool
    // that was already there is never recorded, and one whose installer was only opened (macOS
    // casks) or isn't found yet counts once a later check finds it.
    if let Some(before) = before {
        let after = uninstall::present(dependency).await.unwrap_or(false);
        uninstall::record_outcome(&app, dependency, uninstall::outcome(before, after));
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
#[tauri::command]
pub async fn install_vagrant_plugin(plugin: String, logs: Channel<String>) -> Result<()> {
    if !valid_plugin(&plugin) {
        return Err(Error::Invalid(format!("invalid plugin name `{plugin}`")));
    }
    let mut on_line = channel_log(logs);
    let step = Step { program: "vagrant".into(), args: vec!["plugin".into(), "install".into(), plugin], note: None };
    on_line(command_line(&step));
    run_step(&step, &mut on_line).await?;
    on_line("Done. Re-checking this machine…".into());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [Dependency; 11] = [
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
    ];

    #[test]
    fn only_vagrant_plugin_names_are_installed() {
        assert!(valid_plugin("vagrant-vmware-esxi") && valid_plugin("vagrant_proxmox"));
        assert!(!valid_plugin("vagrant-x; touch /tmp/x") && !valid_plugin("--plugin-source") && !valid_plugin("rails"));
        assert!(!valid_plugin(&format!("vagrant-{}", "a".repeat(64))));
    }

    /// The plan for every dependency on this OS: the ones that install here have steps, the
    /// others say why not.
    fn plans() -> Vec<(Dependency, Result<Vec<Step>>)> {
        ALL.into_iter().map(|d| (d, plan(d))).collect()
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
                Dependency::Libvirt | Dependency::Wsl => assert!(p.is_err(), "{dep:?}"),
                _ => assert!(!p.unwrap().is_empty(), "{dep:?}"),
            }
        }
        let azure = plan(Dependency::Azurecli).unwrap();
        assert_eq!(azure.len(), 3);
        assert!(azure[2].note.is_some() && azure[2].args.contains(&"azure-cli-preview".to_string()));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_installs_through_pkexec_except_the_per_user_gcloud() {
        for (dep, p) in plans() {
            match dep {
                Dependency::Utm | Dependency::Wsl => assert!(p.is_err(), "{dep:?}"),
                Dependency::Gcloud => assert_eq!(p.unwrap()[0].program, "sh"),
                _ => assert_eq!(p.unwrap()[0].program, "pkexec", "{dep:?}"),
            }
        }
        let aws = plan(Dependency::Awscli).unwrap()[0].args.join(" ");
        assert!(aws.contains("awscli-exe-linux") && !aws.contains("/tmp/aws"), "{aws}");
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_installs_through_winget_and_wsl_through_its_installer() {
        for (dep, p) in plans() {
            match dep {
                Dependency::Utm | Dependency::Libvirt => assert!(p.is_err(), "{dep:?}"),
                Dependency::Virtualbox if std::env::consts::ARCH == "aarch64" => assert!(p.is_err()),
                Dependency::Wsl => assert_eq!(p.unwrap()[0].program, "powershell"),
                _ => {
                    let steps = p.unwrap();
                    assert_eq!(steps[0].program, "winget");
                    assert!(steps[0].args.contains(&"--accept-package-agreements".to_string()));
                }
            }
        }
    }

    #[test]
    fn dependencies_use_lowercase_names() {
        assert_eq!(serde_json::to_string(&Dependency::Azurecli).unwrap(), "\"azurecli\"");
        assert_eq!(serde_json::from_str::<Dependency>("\"wsl\"").unwrap(), Dependency::Wsl);
    }

    #[tokio::test]
    async fn an_invalid_plugin_is_refused_before_running_anything() {
        let channel = Channel::new(|_| Ok(()));
        let err = install_vagrant_plugin("rails".into(), channel).await.unwrap_err();
        assert_eq!(err.to_string(), "invalid plugin name `rails`");
    }
}
