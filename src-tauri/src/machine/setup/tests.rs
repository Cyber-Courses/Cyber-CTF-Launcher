use super::shapes::{self, bare};
use super::*;
use crate::machine::system::Tool;
use crate::runtime::providers::{Unavailable, UtilityState};

const BOTH: [SetupPath; 2] = [SetupPath::Docker, SetupPath::Vm];

fn wanted(paths: &[SetupPath], hypervisors: &[Provider]) -> Wanted {
    Wanted { paths: paths.to_vec(), engine: None, hypervisors: hypervisors.to_vec() }
}

fn plan_of(r: &SystemReport, w: &Wanted) -> Plan {
    plan(r, &r.setup, w)
}

/// The plan's steps as `item: action`, short.
fn steps(p: &Plan) -> Vec<String> {
    p.steps
        .iter()
        .map(|s| {
            let what = match &s.action {
                Action::Install { dependency } => format!("install {}", serde_json::to_value(dependency).unwrap().as_str().unwrap()),
                Action::Plugin { plugin } => format!("plugin {plugin}"),
                Action::StartEngine { engine } => format!("start {}", engine.id()),
                Action::UseEngine { engine } => format!("use {}", engine.id()),
                Action::Restart { feature } => format!("restart {feature:?}"),
                Action::Relogin => "relogin".into(),
                Action::Manual { reason, .. } => format!("manual {reason:?}"),
                Action::Blocked { reason } => format!("blocked {reason}"),
            };
            format!("{}: {what}", s.item)
        })
        .collect()
}

fn get_of(r: &SystemReport, provider: Provider) -> Vec<Action> {
    r.setup.hypervisors.iter().find(|h| h.provider == provider).unwrap().get.clone()
}

#[test]
fn a_bare_windows_turns_wsl_on_installs_docker_desktop_virtualbox_and_vagrant_and_passes_over_the_others() {
    let r = shapes::bare_windows();
    let p = plan_of(&r, &wanted(&BOTH, &[Provider::Virtualbox, Provider::Hyperv, Provider::VmwareDesktop]));
    assert_eq!(steps(&p), ["wsl: install wsl", "docker: install docker", "virtualbox: install virtualbox", "vagrant: install vagrant"]);
    assert_eq!(p.hypervisor, Some(Provider::Virtualbox));
    assert_eq!(
        p.passed_over,
        [
            (Provider::Hyperv, "not needed: VirtualBox is set up".to_owned()),
            (Provider::VmwareDesktop, "links out: https://www.vmware.com/products/desktop-hypervisor/workstation-and-fusion".to_owned()),
        ]
    );
    assert_eq!(p.items, ["wsl", "docker", "engine:docker-desktop", "vagrant", "virtualbox"]);
}

#[test]
fn windows_restarts_for_wsl_last_and_installs_everything_else_first() {
    let r = shapes::windows_wsl_waits_for_restart();
    let p = plan_of(&r, &wanted(&BOTH, &[Provider::Virtualbox]));
    assert_eq!(steps(&p), ["docker: install docker", "virtualbox: install virtualbox", "vagrant: install vagrant", "wsl: restart Wsl"]);
    assert!(p.steps.last().unwrap().action.ends_pass());
}

#[test]
fn docker_desktop_is_started_only_once_wsl_is_ready() {
    // Installed, WSL still to turn on: Docker Desktop can't start before the restart.
    let mut r = shapes::bare_windows();
    r.docker = Tool { installed: true, version: Some("Docker version 28.4.0".into()) };
    r.setup = options(&r, false);
    assert_eq!(steps(&plan_of(&r, &wanted(&[SetupPath::Docker], &[]))), ["wsl: install wsl"]);
    // After the restart: started.
    let r = shapes::windows_docker_stopped();
    assert_eq!(steps(&plan_of(&r, &wanted(&[SetupPath::Docker], &[]))), ["engine:docker-desktop: start docker-desktop"]);
}

#[test]
fn a_running_docker_engine_is_proof_enough_of_wsl() {
    // #224: the check couldn't tell, but Docker Desktop runs on WSL 2.
    let mut r = shapes::windows_docker_stopped();
    r.wsl = Some(crate::machine::system::wsl::WslReport { state: WslState::Unknown, reopens_after_restart: true, virtualization_off: false });
    r.docker_running = true;
    r.docker_engine = Some(Engine::DockerDesktop);
    assert_eq!(options(&r, false).wsl, []);
    r.docker_running = false;
    assert_eq!(options(&r, false).wsl, [Action::Install { dependency: Dependency::Wsl }]);
}

#[test]
fn a_bare_debian_installs_docker_engine_libvirt_vagrant_and_its_plugin() {
    let r = shapes::bare_debian();
    let p = plan_of(&r, &wanted(&BOTH, &[Provider::Libvirt, Provider::Virtualbox, Provider::VmwareDesktop]));
    assert_eq!(steps(&p), ["docker: install docker", "libvirt: install libvirt", "vagrant: install vagrant", "plugin:vagrant-libvirt: plugin vagrant-libvirt"]);
    assert_eq!(p.hypervisor, Some(Provider::Libvirt));
    assert_eq!(p.passed_over[0], (Provider::Virtualbox, "not needed: libvirt is set up".to_owned()));
    assert_eq!(p.items, ["docker", "engine:docker-engine", "vagrant", "libvirt", "plugin:vagrant-libvirt"]);
    // Without a choice: the setup window's own, the first hypervisor it installs.
    assert_eq!(plan_of(&r, &wanted(&[SetupPath::Vm], &[])).hypervisor, Some(Provider::Virtualbox));
}

#[test]
fn a_docker_daemon_refusing_this_account_asks_for_a_new_login_last() {
    let r = shapes::debian_docker_denied();
    assert_eq!(r.setup.engines.iter().find(|e| e.engine == Engine::Native).unwrap().actions, [Action::Relogin]);
    let p = plan_of(&r, &wanted(&BOTH, &[Provider::Libvirt]));
    assert_eq!(steps(&p), ["libvirt: install libvirt", "vagrant: install vagrant", "plugin:vagrant-libvirt: plugin vagrant-libvirt", "docker: relogin"]);
}

#[test]
fn installed_libvirt_needs_only_its_plugin_and_one_that_cant_run_is_passed_over() {
    let r = shapes::debian_libvirt_without_plugin();
    assert_eq!(steps(&plan_of(&r, &wanted(&[SetupPath::Vm], &[Provider::Libvirt]))), ["plugin:vagrant-libvirt: plugin vagrant-libvirt"]);
    let r = shapes::debian_libvirt_without_kvm();
    assert!(matches!(get_of(&r, Provider::Libvirt).as_slice(), [Action::Blocked { reason }] if *reason == crate::runtime::providers::Unavailable::NoKvm));
    let p = plan_of(&r, &wanted(&[SetupPath::Vm], &[Provider::Libvirt, Provider::Virtualbox]));
    assert_eq!(p.hypervisor, Some(Provider::Virtualbox));
    assert!(p.passed_over[0].1.starts_with("can't run here: KVM isn't available"), "{:?}", p.passed_over);
    assert_eq!(steps(&p), ["virtualbox: install virtualbox"]);
}

#[test]
fn libvirt_joined_since_logging_in_asks_for_a_new_login_after_the_rest() {
    // VM labs only: the pass installs the plugin, then ends asking for the login that applies the
    // libvirt group (it used to read as ready, and Vagrant was refused by libvirt).
    let r = shapes::debian_libvirt_awaits_relogin();
    assert_eq!(get_of(&r, Provider::Libvirt), [Action::Relogin]);
    let p = plan_of(&r, &wanted(&[SetupPath::Vm], &[Provider::Libvirt]));
    assert_eq!(p.hypervisor, Some(Provider::Libvirt), "a new login is how it is set up, not a reason to pass it over");
    assert_eq!(steps(&p), ["plugin:vagrant-libvirt: plugin vagrant-libvirt", "libvirt: relogin"]);
    // Other groups pending don't hold libvirt.
    let mut r = shapes::debian_libvirt_without_plugin();
    r.groups_pending = vec!["docker".into()];
    assert_eq!(options(&r, false).hypervisors.iter().find(|h| h.provider == Provider::Libvirt).unwrap().get, []);
}

#[test]
fn qemu_without_mkisofs_is_installed_again_which_brings_it() {
    // Regression (the rig's ubuntu2404 attack-box-vm@qemu): QEMU read as set up without the
    // mkisofs its VMs need on Linux, and every VM-form lab failed at its first VM. Its install
    // (which adds genisoimage) is the way: the setup runs it, never passes QEMU over.
    let r = shapes::debian_qemu_without_mkisofs();
    assert_eq!(get_of(&r, Provider::Qemu), [Action::Install { dependency: Dependency::Qemu }]);
    let p = plan_of(&r, &wanted(&[SetupPath::Vm], &[Provider::Qemu]));
    assert_eq!(p.hypervisor, Some(Provider::Qemu));
    assert_eq!(steps(&p), ["qemu: install qemu"]);
    let qemu = r.vm_providers.iter().find(|s| s.provider == Provider::Qemu).unwrap();
    assert_eq!(qemu.reason, Some(Unavailable::MkisofsMissing));
    assert!(!r.ready_hypervisors.contains(&Provider::Qemu));
}

#[test]
fn virtualboxs_driver_waiting_for_the_newer_kernel_ends_the_pass_with_a_restart() {
    // Regression (the rig's bare Debian 13): the install couldn't build VirtualBox's driver for the
    // running kernel, whose headers the archive no longer had; VirtualBox read as ready and the
    // self-test's VM found no driver. Now it waits for the restart into the kernel its install
    // brought, which ends the pass as a restart (`again: reboot`), and it isn't passed over.
    let r = shapes::debian_virtualbox_awaits_restart();
    let restart = Action::Blocked { reason: Unavailable::KernelRestart { kernel: "6.12.116+deb13-amd64".into() } };
    assert_eq!(get_of(&r, Provider::Virtualbox), std::slice::from_ref(&restart));
    assert!(restart.ends_pass());
    let p = plan_of(&r, &wanted(&[SetupPath::Vm], &[Provider::Virtualbox, Provider::Libvirt]));
    assert_eq!(p.hypervisor, Some(Provider::Virtualbox), "a restart is how it is set up, not a reason to pass it over");
    assert_eq!(
        steps(&p),
        [
            "virtualbox: blocked VirtualBox's kernel driver is built for the newer kernel installed with it (6.12.116+deb13-amd64): restart the computer to start that kernel, then re-check."
        ]
    );
    assert!(p.steps.last().unwrap().action.ends_pass());
    // A driver that a restart doesn't mend still can't run here.
    let mut stuck = shapes::debian_virtualbox_awaits_restart();
    for p in stuck.vm_providers.iter_mut().filter(|p| p.provider == Provider::Virtualbox) {
        p.reason = Some(Unavailable::VirtualboxDriver { kernel: "6.12.116+deb13-amd64".into() });
    }
    stuck.setup = options(&stuck, false);
    assert!(!get_of(&stuck, Provider::Virtualbox)[0].ends_pass());
    let p = plan_of(&stuck, &wanted(&[SetupPath::Vm], &[Provider::Virtualbox, Provider::Libvirt]));
    assert!(p.passed_over[0].1.starts_with("can't run here: VirtualBox's kernel driver (vboxdrv) isn't loaded"), "{:?}", p.passed_over);
}

/// `r` with VirtualBox's status reading `reason` (installed or not, as `r` has it), the options
/// worked out again.
fn virtualbox_reads(mut r: SystemReport, reason: &Unavailable) -> SystemReport {
    for p in r.vm_providers.iter_mut().filter(|p| p.provider == Provider::Virtualbox) {
        (p.available, p.reason) = (false, Some(reason.clone()));
    }
    r.setup = options(&r, false);
    r
}

#[test]
fn the_setups_own_hypervisor_that_cant_run_here_gives_way_to_one_that_can() {
    // Regression (the rig's debian13-bare setup-full, pass 3): VirtualBox, the setup's own, was
    // installed but its driver wouldn't load after the restart, and the setup failed with "none of
    // the offered ones can be set up here" while libvirt and QEMU could be.
    let stuck = virtualbox_reads(shapes::debian_virtualbox_awaits_restart(), &Unavailable::VirtualboxDriver { kernel: "6.12.116+deb13-amd64".into() });
    assert_eq!(stuck.setup.hypervisor, Some(Provider::Virtualbox), "still the one installed, as the window shows it");
    let p = plan_of(&stuck, &wanted(&[SetupPath::Vm], &[]));
    assert_eq!(p.hypervisor, Some(Provider::Libvirt), "{:?}", p.passed_over);
    assert!(p.passed_over[0].0 == Provider::Virtualbox && p.passed_over[0].1.starts_with("can't run here: VirtualBox's kernel driver"), "{:?}", p.passed_over);
    assert!(steps(&p).contains(&"libvirt: install libvirt".to_owned()), "{:?}", steps(&p));
    // One named by the player is still theirs alone: no other is set up in its place.
    assert_eq!(plan_of(&stuck, &wanted(&[SetupPath::Vm], &[Provider::Virtualbox])).hypervisor, None);
}

#[test]
fn virtualbox_is_never_installed_while_kvm_holds_the_cpus_virtualization() {
    // Regression (the rig's debian13-bare setup-full, pass 1): Linux 6.12 turns VT-x on as KVM's
    // module loads, so VirtualBox can't start a VM ("VT-x is being used by another hypervisor");
    // the setup installed it anyway, asked for a restart for its driver, then dropped it.
    let r = virtualbox_reads(shapes::bare_debian(), &Unavailable::KvmHoldsVirtualization);
    let held = Action::Blocked { reason: Unavailable::KvmHoldsVirtualization };
    assert_eq!(get_of(&r, Provider::Virtualbox), std::slice::from_ref(&held));
    assert!(!held.ends_pass(), "no restart mends it");
    assert_eq!(r.setup.hypervisor, Some(Provider::Libvirt), "the window starts on one that can run here");
    let p = plan_of(&r, &wanted(&BOTH, &[]));
    assert_eq!(p.hypervisor, Some(Provider::Libvirt));
    assert!(!steps(&p).iter().any(|s| s.starts_with("virtualbox:")), "{:?}", steps(&p));
    assert!(!p.steps.iter().any(|s| s.action.ends_pass() && s.item == "virtualbox"), "no restart for VirtualBox");
    // Installed already: blocked too, and nothing of Vagrant's is set up for it.
    let installed = virtualbox_reads(with_virtualbox(shapes::bare_debian()), &Unavailable::KvmHoldsVirtualization);
    assert_eq!(get_of(&installed, Provider::Virtualbox), [held]);
    assert_eq!(plan_of(&installed, &wanted(&[SetupPath::Vm], &[])).hypervisor, Some(Provider::Libvirt));
}

/// `r` with VirtualBox installed (Vagrant and its needs as `r` has them).
fn with_virtualbox(mut r: SystemReport) -> SystemReport {
    for p in r.vm_providers.iter_mut().filter(|p| p.provider == Provider::Virtualbox) {
        p.hypervisor = Some(true);
    }
    r
}

#[test]
fn qemu_joined_to_the_kvm_group_asks_for_a_new_login_before_its_self_test() {
    // Regression: the QEMU install never added the player to the kvm group (Debian 13's /dev/kvm
    // is root:kvm 0660), and nothing asked for the login that applies it: the self-test fell back
    // to software emulation and every lab's `accel=kvm` was refused.
    let r = shapes::debian_qemu_awaits_relogin();
    assert_eq!(get_of(&r, Provider::Qemu), [Action::Relogin]);
    let p = plan_of(&r, &wanted(&[SetupPath::Vm], &[Provider::Qemu]));
    assert_eq!(p.hypervisor, Some(Provider::Qemu), "a new login is how it is set up, not a reason to pass it over");
    assert_eq!(steps(&p).last().map(String::as_str), Some("qemu: relogin"), "{:?}", steps(&p));
    // On a Mac QEMU runs on HVF: a group pending is no reason to log in again.
    let mut mac = shapes::debian_qemu_awaits_relogin();
    mac.os = "macos";
    for p in mac.vm_providers.iter_mut().filter(|p| p.provider == Provider::Qemu) {
        (p.available, p.reason, p.account_lacks) = (true, None, None);
    }
    assert_eq!(options(&mac, false).hypervisors.iter().find(|h| h.provider == Provider::Qemu).unwrap().get, []);
}

#[test]
fn a_linux_without_apt_is_told_to_install_each_tool_itself_instead_of_a_failing_apt_command() {
    // Regression: Fedora, openSUSE and Arch were offered the apt installs, which failed.
    let r = shapes::bare_fedora();
    let distribution = |dependency, url: &str| Action::Manual { reason: ManualReason::Distribution, dependency: Some(dependency), url: Some(url.into()) };
    assert_eq!(
        r.setup.engines.iter().find(|e| e.engine == Engine::Native).unwrap().actions,
        [distribution(Dependency::Docker, "https://docs.docker.com/engine/install/")]
    );
    assert_eq!(get_of(&r, Provider::Libvirt), [distribution(Dependency::Libvirt, "https://libvirt.org/")]);
    assert_eq!(get_of(&r, Provider::Virtualbox), [distribution(Dependency::Virtualbox, "https://www.virtualbox.org/wiki/Linux_Downloads")]);
    let p = plan_of(&r, &wanted(&BOTH, &[Provider::Libvirt, Provider::Virtualbox]));
    assert_eq!(steps(&p), ["docker: manual Distribution"]);
    assert_eq!(p.hypervisor, None);
    assert_eq!(
        p.passed_over,
        [
            (Provider::Libvirt, "this distribution isn't supported by the guided setup yet: install libvirt yourself".to_owned()),
            (Provider::Virtualbox, "this distribution isn't supported by the guided setup yet: install VirtualBox yourself".to_owned()),
        ]
    );
    // Vagrant too, once a hypervisor is there.
    let mut r = shapes::bare_fedora();
    for p in r.vm_providers.iter_mut().filter(|p| p.provider == Provider::Libvirt) {
        p.hypervisor = Some(true);
    }
    r.setup = options(&r, false);
    let vagrant = &r.setup.hypervisors.iter().find(|h| h.provider == Provider::Libvirt).unwrap().vagrant;
    assert_eq!(vagrant[0], distribution(Dependency::Vagrant, "https://developer.hashicorp.com/vagrant/install"));
}

#[test]
fn a_mac_with_docker_and_no_vagrant_sets_up_only_the_vm_path() {
    let r = shapes::mac_docker_without_vagrant();
    assert_eq!(plan_of(&r, &wanted(&[SetupPath::Docker], &[])).steps, []);
    // The setup's own hypervisor: the first it installs on Apple Silicon (VMware and Parallels link out).
    assert_eq!(r.setup.hypervisor, Some(Provider::Qemu));
    let p = plan_of(&r, &wanted(&BOTH, &[Provider::Utm]));
    assert_eq!(steps(&p), ["utm: install utm", "vagrant: install vagrant", "plugin:vagrant_utm: plugin vagrant_utm"]);
    assert_eq!(p.items, ["docker", "engine:docker-desktop", "vagrant", "utm", "plugin:vagrant_utm"]);
}

#[test]
fn vmware_gets_its_plugin_and_hashicorps_utility_and_its_install_links_out() {
    let r = shapes::mac_vmware_without_its_parts();
    let p = plan_of(&r, &wanted(&[SetupPath::Vm], &[Provider::VmwareDesktop]));
    assert_eq!(steps(&p), ["plugin:vagrant-vmware-desktop: plugin vagrant-vmware-desktop", "vmware-utility: install vmware-utility"]);
    assert_eq!(p.items, ["vagrant", "vmware_desktop", "plugin:vagrant-vmware-desktop", "vmware-utility"]);
    assert!(matches!(get_of(&shapes::bare_windows(), Provider::VmwareDesktop).as_slice(), [Action::Manual { reason: ManualReason::Vendor, url: Some(_), .. }]));
    // A stopped utility is installed again (which restarts it).
    let mut r = shapes::mac_vmware_without_its_parts();
    for p in r.vm_providers.iter_mut().filter(|p| p.provider == Provider::VmwareDesktop) {
        p.utility = Some(UtilityState::Stopped);
    }
    r.setup = options(&r, false);
    assert!(r.setup.hypervisors.iter().any(|h| h.vagrant.contains(&Action::Install { dependency: Dependency::VmwareUtility })));
}

#[test]
fn hyper_v_is_turned_on_by_the_setup_then_waits_for_a_restart() {
    // Was a link to Microsoft's guide.
    assert_eq!(get_of(&shapes::bare_windows(), Provider::Hyperv), [Action::Install { dependency: Dependency::Hyperv }]);
    let r = shapes::windows_hyperv_waits_for_restart();
    assert_eq!(get_of(&r, Provider::Hyperv), [Action::Restart { feature: WindowsFeature::Hyperv }]);
    let p = plan_of(&r, &wanted(&[SetupPath::Vm], &[Provider::Hyperv]));
    assert_eq!(p.hypervisor, Some(Provider::Hyperv));
    assert_eq!(steps(&p), ["vagrant: install vagrant", "hyperv: restart Hyperv"]);
}

#[test]
fn each_engine_says_what_makes_it_the_one_docker_uses() {
    let r = shapes::mac_two_engines();
    let of = |e: Engine| r.setup.engines.iter().find(|o| o.engine == e).unwrap().actions.clone();
    assert_eq!(r.setup.engines.iter().map(|o| o.engine).collect::<Vec<_>>(), [Engine::DockerDesktop, Engine::Orbstack, Engine::Colima]);
    assert!(of(Engine::Orbstack).is_empty(), "in use");
    assert_eq!(of(Engine::DockerDesktop), [Action::UseEngine { engine: Engine::DockerDesktop }]);
    assert_eq!(of(Engine::Colima), [Action::Manual { reason: ManualReason::Vendor, dependency: None, url: Some("https://github.com/abiosoft/colima".into()) }]);
    assert_eq!(r.setup.engine, Engine::Orbstack);
    // A plan for another engine: switched to.
    let w = Wanted { engine: Some(Engine::DockerDesktop), ..wanted(&[SetupPath::Docker], &[]) };
    assert_eq!(steps(&plan(&r, &r.setup, &w)), ["engine:docker-desktop: use docker-desktop"]);
    // Nothing installed: the recommended one is installed, the others link out.
    let r = bare("macos", "x86_64");
    assert_eq!(r.setup.recommended_engine, Engine::DockerDesktop);
    assert_eq!(r.setup.engines[0].actions, [Action::Install { dependency: Dependency::Docker }]);
    assert!(matches!(r.setup.engines[1].actions.as_slice(), [Action::Manual { reason: ManualReason::Vendor, .. }]));
    assert_eq!(shapes::bare_debian().setup.recommended_engine, Engine::Native);
}

#[test]
fn the_setups_own_hypervisor_is_an_installed_one_that_runs_else_installed_else_installable() {
    assert_eq!(shapes::mac_ready().setup.hypervisor, Some(Provider::Utm));
    // libvirt installed but unable to run, VirtualBox not installed: still libvirt (installed).
    assert_eq!(shapes::debian_libvirt_without_kvm().setup.hypervisor, Some(Provider::Libvirt));
    assert_eq!(shapes::bare_debian().setup.hypervisor, Some(Provider::Virtualbox));
    // Nothing the setup installs (Fedora): the first.
    assert_eq!(shapes::bare_fedora().setup.hypervisor, Some(Provider::Virtualbox));
}

#[test]
fn a_hypervisor_not_offered_here_is_passed_over() {
    let p = plan_of(&shapes::bare_debian(), &wanted(&[SetupPath::Vm], &[Provider::Hyperv, Provider::Libvirt]));
    assert_eq!(p.passed_over, [(Provider::Hyperv, "not offered on this machine".to_owned())]);
    assert_eq!(p.hypervisor, Some(Provider::Libvirt));
}

#[test]
fn a_ready_machine_has_nothing_to_do() {
    let r = shapes::mac_ready();
    let p = plan_of(&r, &wanted(&BOTH, &[Provider::Utm]));
    assert!(p.steps.is_empty(), "{:?}", steps(&p));
    assert_eq!(p.items, ["docker", "engine:docker-desktop", "vagrant", "utm", "plugin:vagrant_utm"]);
}

#[test]
fn actions_reach_the_webview_tagged_by_kind() {
    let json = |a: Action| serde_json::to_value(a).unwrap();
    assert_eq!(json(Action::Install { dependency: Dependency::VmwareUtility }), serde_json::json!({ "kind": "install", "dependency": "vmware-utility" }));
    assert_eq!(json(Action::StartEngine { engine: Engine::Native }), serde_json::json!({ "kind": "startEngine", "engine": "docker-engine" }));
    assert_eq!(json(Action::Relogin), serde_json::json!({ "kind": "relogin" }));
    assert_eq!(json(Action::Restart { feature: WindowsFeature::Hyperv }), serde_json::json!({ "kind": "restart", "feature": "hyperv" }));
    assert_eq!(
        json(Action::Manual { reason: ManualReason::Distribution, dependency: Some(Dependency::Vagrant), url: None }),
        serde_json::json!({ "kind": "manual", "reason": "distribution", "dependency": "vagrant", "url": null })
    );
    let paths: Vec<SetupPath> = serde_json::from_str(r#"["docker","vm"]"#).unwrap();
    assert_eq!(paths, BOTH);
}
