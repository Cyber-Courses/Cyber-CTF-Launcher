use super::*;
use crate::platform::host::LinuxPackages;

/// Regression (a clean Windows 11): `winget install -e --id Docker.DockerDesktop` without a source
/// failed with 0x8a15005e, the Microsoft Store source's certificate pinning failing, in both
/// passes. winget is asked of its own source alone.
#[test]
fn winget_installs_and_removes_from_its_own_source_alone() {
    let install = winget("install", "Docker.DockerDesktop", &["--accept-source-agreements", "--accept-package-agreements"]);
    assert_eq!(command_line(&install), "$ winget install -e --id Docker.DockerDesktop --source winget --accept-source-agreements --accept-package-agreements");
    assert_eq!(command_line(&winget("uninstall", "Hashicorp.Vagrant", &[])), "$ winget uninstall -e --id Hashicorp.Vagrant --source winget");
}

#[test]
fn docker_is_named_as_the_setup_installs_it_on_each_os() {
    assert_eq!(Dependency::Docker.name_on(Os::Linux), "Docker Engine");
    assert_eq!(Dependency::Docker.name_on(Os::Macos), "Docker Desktop");
    assert_eq!(Dependency::Docker.name_on(Os::Windows), "Docker Desktop");
    assert_eq!(Dependency::Awscli.name_on(Os::Linux), "AWS CLI");
}

#[test]
fn each_tool_runs_on_its_os_and_is_refused_elsewhere_in_one_message() {
    for (dep, oses) in [
        (Dependency::Utm, [Os::Macos].as_slice()),
        (Dependency::Libvirt, &[Os::Linux]),
        (Dependency::Wsl, &[Os::Windows]),
        (Dependency::Hyperv, &[Os::Windows]),
        (Dependency::Docker, &[Os::Macos, Os::Linux, Os::Windows]),
        (Dependency::Qemu, &[Os::Macos, Os::Linux]),
    ] {
        for os in Os::ALL {
            assert_eq!(dep.runs_on(os), oses.contains(&os), "{dep:?} on {os:?}");
        }
    }
    // Every tool an OS doesn't have is refused there before any step, saying so, and its
    // removal the same way.
    for host in hosts() {
        for dep in Dependency::ALL.into_iter().filter(|d| !d.runs_on(host.os)) {
            let said = plan_on(dep, host, None).err().map(|e| e.to_string()).unwrap_or_default();
            assert_eq!(said, super::not_here(dep, host.os).to_string(), "{host:?}");
            assert!(said.starts_with(&format!("{} doesn't run on {}", dep.name_on(host.os), host.os.name())), "{said}");
            assert_eq!(uninstall::plan_on(dep, host, None).err().map(|e| e.to_string()), Some(said), "{host:?}");
        }
    }
}

/// Every computer the guided setup knows: each OS and CPU, with and without its package
/// manager (Linux's two).
fn hosts() -> Vec<Host> {
    let mut hosts = Vec::new();
    for arch in Arch::ALL {
        for (os, packages) in [
            (Os::Macos, Some(Packages::Homebrew(BREW))),
            (Os::Macos, None),
            (Os::Windows, Some(Packages::Winget)),
            (Os::Linux, Some(Packages::Linux(LinuxPackages::Apt))),
            (Os::Linux, Some(Packages::Linux(LinuxPackages::Dnf))),
            (Os::Linux, None),
        ] {
            hosts.push(Host { os, arch, packages });
        }
    }
    hosts
}

const BREW: &str = "/opt/homebrew/bin/brew";
const MAC: Host = Host { os: Os::Macos, arch: Arch::Aarch64, packages: Some(Packages::Homebrew(BREW)) };
const WINDOWS: Host = Host { os: Os::Windows, arch: Arch::X86_64, packages: Some(Packages::Winget) };
const APT: Host = Host { os: Os::Linux, arch: Arch::X86_64, packages: Some(Packages::Linux(LinuxPackages::Apt)) };
const DNF: Host = Host { os: Os::Linux, arch: Arch::X86_64, packages: Some(Packages::Linux(LinuxPackages::Dnf)) };
const BARE_LINUX: Host = Host { os: Os::Linux, arch: Arch::X86_64, packages: None };

#[test]
fn every_tool_on_every_computer_installs_or_says_why_and_each_install_has_a_way_out() {
    for host in hosts() {
        for dep in Dependency::ALL {
            let install = plan_on(dep, host, Some("ada"));
            let removal = uninstall::plan_on(dep, host, Some("/x/VirtualBox.dmg"));
            if let Ok(steps) = &install {
                assert!(!steps.is_empty(), "{dep:?} on {host:?}");
                // A Windows feature is turned off by the player in Windows Features.
                if !matches!(dep, Dependency::Wsl | Dependency::Hyperv) {
                    let why = removal.as_ref().err().map(ToString::to_string);
                    assert!(removal.as_ref().is_ok_and(|r| !r.is_empty()), "{dep:?} on {host:?} installs but has no removal: {why:?}");
                }
            }
            if let Err(e) = &install {
                assert!(!e.to_string().is_empty(), "{dep:?} on {host:?}");
            }
        }
    }
}

#[test]
fn every_local_hypervisor_is_installed_by_the_setup_or_linked_to_its_vendor() {
    for p in Provider::ALL.into_iter().filter(|p| !p.is_remote()) {
        assert!(Dependency::of_hypervisor(p).is_some() || p.vendor().is_some(), "{p:?}: neither installed nor linked");
    }
}

#[test]
fn a_hypervisor_is_installed_exactly_where_it_runs_and_its_provider_names_the_tool() {
    // Regression (#314): QEMU was installed through winget on Windows, where no lab runs on it.
    for dep in Dependency::ALL {
        let Some(provider) = dep.hypervisor() else { continue };
        assert_eq!(Dependency::of_hypervisor(provider), Some(dep), "{provider:?}");
        for os in Os::ALL {
            assert_eq!(dep.runs_on(os), provider.runs_on(os), "{dep:?} on {os:?}");
        }
    }
    assert!(plan_on(Dependency::Qemu, WINDOWS, None).is_err());
    assert!(Dependency::Qemu.row().winget.is_none());
}

#[test]
fn windows_on_arm_is_refused_the_hypervisors_not_built_for_it_by_the_hypervisor_table() {
    let arm = Host { arch: Arch::Aarch64, ..WINDOWS };
    let said = |dep| plan_on(dep, arm, None).err().map(|e| e.to_string()).unwrap_or_default();
    assert_eq!(said(Dependency::Virtualbox), "VirtualBox doesn't run on Windows on Arm. Run VM labs with Hyper-V, on a server or in the cloud.");
    assert_eq!(said(Dependency::VmwareUtility), "VMware Workstation doesn't run on Windows on Arm. Run VM labs with Hyper-V, on a server or in the cloud.");
    assert!(plan_on(Dependency::Hyperv, arm, None).is_ok() && plan_on(Dependency::Docker, arm, None).is_ok());
}

#[tokio::test]
async fn the_policy_refuses_every_one_click_install_before_anything_runs() {
    let _policy = policy::Override::with(&[Key::DisableToolInstalls]);
    let tools = crate::platform::exec::fake::Tools::strict_rules(&[]);
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
fn only_vagrant_plugin_names_are_installed() {
    assert!(valid_plugin("vagrant-vmware-esxi") && valid_plugin("vagrant_proxmox"));
    assert!(!valid_plugin("vagrant-x; touch /tmp/x") && !valid_plugin("--plugin-source") && !valid_plugin("rails"));
    assert!(!valid_plugin(&format!("vagrant-{}", "a".repeat(64))));
}

/// The plan for every dependency on `host`: the ones that install there have steps, the
/// others say why not.
fn plans(host: Host) -> Vec<(Dependency, Result<Vec<Step>>)> {
    Dependency::ALL.into_iter().map(|d| (d, plan_on(d, host, Some("ada")).map(own))).collect()
}

/// The launcher's own steps of `plan` (macOS's, Windows').
fn own(plan: Plan) -> Vec<Step> {
    match plan {
        Plan::Own(steps) => steps,
        Plan::Machine(_) => panic!("isothrum-machine's steps"),
    }
}

#[test]
fn macos_installs_through_homebrew() {
    let without = Host { packages: None, ..MAC };
    assert!(plan_on(Dependency::Docker, without, None).err().unwrap().to_string().contains("Homebrew is required"));
    assert!(plan_on(Dependency::VmwareUtility, without, None).is_ok(), "HashiCorp's own package");
    for (dep, p) in plans(MAC) {
        match dep {
            Dependency::Libvirt | Dependency::Wsl | Dependency::Hyperv => assert!(p.is_err(), "{dep:?}"),
            _ => assert!(!p.unwrap().is_empty(), "{dep:?}"),
        }
    }
    let azure = own(plan_on(Dependency::Azurecli, MAC, None).unwrap());
    assert_eq!(azure.len(), 3);
    assert!(azure[2].note.is_some() && azure[2].args.contains(&"azure-cli-preview".to_string()));
    // QEMU is one shell step: a best-effort Homebrew update that never aborts the install
    // (`|| …`), then the install itself. It must not be a second, fatal `brew update` step.
    let qemu = own(plan_on(Dependency::Qemu, MAC, None).unwrap());
    assert_eq!(qemu.len(), 1);
    assert_eq!(qemu[0].program, "sh");
    let script = qemu[0].args.join(" ");
    assert!(script.contains("update ||") && script.contains("install qemu"), "{script}");
}

#[test]
fn windows_installs_through_winget_and_wsl_through_its_installer() {
    for (dep, p) in plans(WINDOWS) {
        match dep {
            Dependency::Utm | Dependency::Libvirt | Dependency::Qemu => assert!(p.is_err(), "{dep:?}"),
            Dependency::Wsl | Dependency::VmwareUtility | Dependency::Hyperv => assert_eq!(p.unwrap()[0].program, "powershell"),
            _ => {
                let steps = p.unwrap();
                assert_eq!(steps[0].program, "winget");
                assert!(steps[0].args.contains(&"--accept-package-agreements".to_string()));
                assert!(steps[0].args.windows(2).any(|w| w == ["--source", "winget"]), "{dep:?}: {:?}", steps[0].args);
            }
        }
    }
}

#[test]
fn a_distribution_without_apt_or_dnf_is_told_to_install_it_itself_before_anything_runs() {
    // Regression: Fedora, openSUSE and Arch got `apt-get: not found` from a pkexec prompt.
    for dep in [Dependency::Vagrant, Dependency::Virtualbox, Dependency::Libvirt, Dependency::Qemu, Dependency::Docker] {
        let err = plan_on(dep, BARE_LINUX, None).err().unwrap_or_else(|| panic!("{dep:?} refused"));
        assert!(err.to_string().starts_with("This distribution isn't supported by the guided setup yet (it has neither apt nor dnf): install "), "{err}");
        assert!(plan_on(dep, APT, None).is_ok(), "{dep:?}");
    }
    assert!(plan_on(Dependency::Utm, BARE_LINUX, None).err().is_some_and(|e| e.to_string().starts_with("UTM doesn't run on Linux")));
}

#[test]
fn without_apt_or_dnf_the_install_says_the_distribution_isnt_supported_yet_and_what_to_install() {
    let err = plan_on(Dependency::Virtualbox, BARE_LINUX, None).err().unwrap().to_string();
    assert_eq!(err, "This distribution isn't supported by the guided setup yet (it has neither apt nor dnf): install VirtualBox yourself, then re-check.");
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
    if Os::HERE == Os::Windows { Dependency::Utm } else { Dependency::Wsl }
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
    let tools = Tools::strict_rules(&[]);
    let log = Sink::default();
    let err = install_dependency(t.dirs(), not_here(), log.on_line()).await.unwrap_err();
    assert_eq!(log.lines(), [err.to_string()]);
    assert!(tools.calls().is_empty(), "{:?}", tools.calls());
    assert!(uninstall::installed_tools(t.dirs()).await.is_empty());
}

#[tokio::test]
async fn an_install_streams_its_steps_and_records_a_tool_it_added() {
    let t = TestApp::new();
    let tools = terraform_tools(Rc::new(Cell::new(false)), true);
    let log = Sink::default();
    let steps = match plan(Dependency::Terraform) {
        Ok(steps) => steps,
        // A host with nothing to install through (macOS without Homebrew, Linux without apt or
        // dnf): the install is refused as its plan is, before anything runs.
        Err(refused) => {
            let err = install_dependency(t.dirs(), Dependency::Terraform, log.on_line()).await.unwrap_err();
            assert_eq!(err.to_string(), refused.to_string());
            assert!(tools.calls().is_empty(), "{:?}", tools.calls());
            return;
        }
    };
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

#[test]
fn a_root_step_prompts_unless_unattended_where_it_runs_as_root_or_through_sudo_n() {
    use isothrum_machine::install::Elevation;
    assert_eq!(super::linux_root().elevation, Elevation::Prompt, "the app prompts whoever runs it");
    crate::platform::elevation::set_unattended(true);
    let unattended = super::linux_root().elevation;
    crate::platform::elevation::set_unattended(false);
    assert_eq!(unattended, Elevation::Unattended { root: crate::platform::elevation::is_root() });
}

#[test]
fn linux_installs_and_removals_are_isothrum_machines_said_in_the_players_words() {
    // isothrum-machine's root scripts, through pkexec; every plan's steps and refusals are tested there.
    for (dep, plan) in Dependency::ALL.into_iter().map(|d| (d, plan_on(d, APT, Some("ada")))) {
        match dep {
            Dependency::Utm | Dependency::Wsl | Dependency::Hyperv => assert!(plan.is_err(), "{dep:?}"),
            _ => assert_eq!(plan.unwrap().commands()[0].0, "pkexec", "{dep:?}"),
        }
    }
    let apt_only = plan_on(Dependency::Gcloud, DNF, None).err().unwrap().to_string();
    assert_eq!(apt_only, "The guided setup installs Google Cloud CLI through apt only for now: install it yourself, then re-check.");
    let removal = uninstall::plan_on(Dependency::Vagrant, DNF, None).unwrap().commands();
    assert!(removal[0].1.last().unwrap().ends_with("dnf remove -y vagrant"), "{removal:?}");
}
