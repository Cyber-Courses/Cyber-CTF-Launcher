//! Assisted, one-click install of lab dependencies, streaming the package manager's output.
//! It never shows a custom password prompt: it uses the OS's own trusted flow (winget's
//! UAC, Linux pkexec, or downloading the official app/installer for macOS casks and
//! letting the tool's own installer run).

use serde::Deserialize;

#[cfg(target_os = "macos")]
use super::scripts::fetch_and_open;
#[cfg(target_os = "linux")]
use super::scripts::{TERRAFORM_SCRIPT, VIRTUALBOX_SCRIPT, awscli_script, azurecli_script, docker_script, gcloud_script};
use super::steps::{Step, command_line, run_step, run_steps, step};
use super::uninstall;
use crate::error::{Error, Result};

#[derive(Deserialize, serde::Serialize, Clone, Copy, PartialEq, Debug, ts_rs::TS)]
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

/// The install steps on macOS: Homebrew formulae, or a cask whose installer the user finishes.
#[cfg(target_os = "macos")]
fn macos_plan(dep: Dependency) -> Result<Vec<Step>> {
    let brew = crate::platform::brew_bin().ok_or_else(|| Error::Invalid("Homebrew is required. Install it from https://brew.sh, then try again.".into()))?;
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
            // `brew trust` exists from Homebrew 7 only; an older Homebrew needs no trust and
            // must not fail here ("Unknown command"). On 7, an untrusted tap still fails at
            // the install step with Homebrew's own message.
            Step {
                program: "sh".into(),
                args: vec!["-c".into(), format!("'{brew}' trust azure/azure-cli 2>/dev/null || true")],
                note: Some("Trusting the azure/azure-cli tap (Homebrew 7 asks for it; older versions don't).".into()),
            },
            Step {
                program: brew.clone(),
                args: vec!["install".into(), "--cask".into(), "azure-cli-preview".into()],
                note: Some("Installing the prebuilt Azure CLI cask (avoids a long source build on this macOS).".into()),
            },
        ],
        Dependency::Gcloud => vec![step(brew.clone(), &["install", "--cask", "google-cloud-sdk"])],
    })
}

/// The install step on Windows: a winget package, or WSL's own elevated installer.
#[cfg(target_os = "windows")]
fn windows_plan(dep: Dependency) -> Result<Vec<Step>> {
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
            // `-PassThru` surfaces wsl.exe's own exit code (without it PowerShell exits 0
            // whatever happened); a declined UAC prompt throws, reported as ERROR_CANCELLED.
            return Ok(vec![step(
                "powershell",
                &[
                    "-NoProfile",
                    "-Command",
                    "try { $p = Start-Process -FilePath wsl.exe -ArgumentList '--install','--no-distribution' -Verb RunAs -Wait -PassThru; exit $p.ExitCode } catch { exit 1223 }",
                ],
            )]);
        }
    };
    Ok(vec![step("winget", &["install", "-e", "--id", id, "--accept-source-agreements", "--accept-package-agreements"])])
}

/// The install steps on Linux: apt and the vendors' installers, as root through pkexec.
#[cfg(target_os = "linux")]
fn linux_plan(dep: Dependency) -> Result<Vec<Step>> {
    // pkexec raises a graphical password prompt (polkit) for the privileged install.
    let root = |script: String| Step { program: "pkexec".into(), args: vec!["sh".into(), "-c".into(), script], note: None };
    Ok(match dep {
        Dependency::Wsl => return Err(Error::Invalid("WSL is only available on Windows.".into())),
        Dependency::Docker => vec![root(docker_script(crate::platform::exec::login_name().as_deref()))],
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
        Dependency::Gcloud => vec![step("sh", &["-c", &gcloud_script()])],
    })
}

/// The ordered install steps for a dependency on this OS.
fn plan(dep: Dependency) -> Result<Vec<Step>> {
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

/// Installs a dependency, streaming each command and its output to the UI.
pub async fn install_dependency(app: crate::AppHandle, dependency: Dependency, mut on_line: impl FnMut(String)) -> Result<()> {
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
        uninstall::record_outcome(&app, dependency, uninstall::outcome(before, after));
    }
    // Its features start at the next restart: the check asks for one until then, even if the
    // app is reopened first.
    if dependency == Dependency::Wsl {
        crate::machine::system::wsl::mark_installed(&app);
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
        let err = install_dependency(t.handle().clone(), not_here(), log.on_line()).await.unwrap_err();
        assert_eq!(log.lines(), [err.to_string()]);
        assert!(tools.calls().is_empty(), "{:?}", tools.calls());
        assert!(uninstall::installed_tools(t.handle().clone()).await.is_empty());
    }

    #[tokio::test]
    async fn an_install_streams_its_steps_and_records_a_tool_it_added() {
        let Ok(steps) = plan(Dependency::Terraform) else { return }; // macOS without Homebrew
        let t = TestApp::new();
        let tools = terraform_tools(Rc::new(Cell::new(false)), true);
        let log = Sink::default();
        install_dependency(t.handle().clone(), Dependency::Terraform, log.on_line()).await.unwrap();
        let lines = log.lines();
        assert_eq!(lines.first(), Some(&command_line(&steps[0])), "{lines:?}");
        assert!(lines.contains(&"Setting up terraform (1.9.8-1) ...".to_string()), "{lines:?}");
        assert_eq!(lines.last().map(String::as_str), Some("Done. Re-checking this machine…"));
        assert!(tools.called(&format!("{} {}", steps[0].program, steps[0].args.join(" "))));
        let ours: Vec<_> = uninstall::installed_tools(t.handle().clone()).await.into_iter().map(|t| t.dependency).collect();
        assert_eq!(ours, [Dependency::Terraform]);

        // Cleanup removes it, then forgets it: a second removal is refused.
        let log = Sink::default();
        uninstall::uninstall_dependency(t.handle().clone(), Dependency::Terraform, log.on_line()).await.unwrap();
        assert_eq!(log.lines().last().map(String::as_str), Some("Removed."));
        assert!(uninstall::installed_tools(t.handle().clone()).await.is_empty());
        let err = uninstall::uninstall_dependency(t.handle().clone(), Dependency::Terraform, Sink::default().on_line()).await.unwrap_err();
        assert_eq!(err.to_string(), "Cyber CTF didn't install this tool, so it won't remove it.");
    }

    #[tokio::test]
    async fn a_tool_that_was_already_there_is_never_recorded() {
        if plan(Dependency::Terraform).is_err() {
            return;
        }
        let t = TestApp::new();
        let _tools = terraform_tools(Rc::new(Cell::new(true)), true);
        install_dependency(t.handle().clone(), Dependency::Terraform, Sink::default().on_line()).await.unwrap();
        assert!(uninstall::installed_tools(t.handle().clone()).await.is_empty());
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
        install_dependency(t.handle().clone(), Dependency::Terraform, Sink::default().on_line()).await.unwrap();
        assert!(uninstall::installed_tools(t.handle().clone()).await.is_empty(), "pending, not shown");
        present.set(true);
        let ours: Vec<_> = uninstall::installed_tools(t.handle().clone()).await.into_iter().map(|t| t.dependency).collect();
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
        let err = install_dependency(t.handle().clone(), Dependency::Terraform, log.on_line()).await.unwrap_err();
        assert!(err.to_string().contains("exited with"), "{err}");
        assert!(log.lines().contains(&"E: Unable to locate package terraform".to_string()));
        assert!(!log.lines().iter().any(|l| l.starts_with("Done.")));
        assert!(uninstall::installed_tools(t.handle().clone()).await.is_empty());
    }

    #[tokio::test]
    async fn an_invalid_plugin_is_refused_before_running_anything() {
        let err = install_vagrant_plugin("rails".into(), |_| {}).await.unwrap_err();
        assert_eq!(err.to_string(), "invalid plugin name `rails`");
    }
}
