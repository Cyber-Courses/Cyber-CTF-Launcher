use super::*;

/// A lab with both forms: a container per machine, and a VM recipe for it.
const DUAL: &str = "version: 1\nname: t\nnetworks: { lab: { cidr: 10.70.0.0/24 } }\nmachines:\n  web: { os: debian@12, app: { steps: [{ run: web.sh }] }, networks: { lab: 10 } }\n";

/// A VM lab: a Windows machine.
const VMS: &str = "version: 1\nname: ad\nnetworks: { lan: { cidr: 10.20.0.0/24 } }\nmachines:\n  dc: { os: windows-server@2022, networks: { lan: 10 } }\n";

/// The lab's run options on an x86-64 machine (the machines below are built for it).
fn options(spec: &str, runtime: Runtime) -> RunOptions {
    options_on(spec, runtime, "x86_64")
}

/// The lab's run options on a `host` CPU.
fn options_on(spec: &str, runtime: Runtime, host: &str) -> RunOptions {
    run_options_on(&Lab::new(isoloom_spec::parse(spec).unwrap()), runtime, host)
}

/// The providers of `paths`, in order.
fn providers(paths: &[RunPath]) -> Vec<Provider> {
    paths.iter().filter_map(|p| p.provider).collect()
}

/// Each path's `(provider, target, variant)`.
fn paths_of(paths: &[RunPath]) -> Vec<(Option<Provider>, &str, Option<&str>)> {
    paths.iter().map(|p| (p.provider, p.target.as_str(), p.variant.as_deref())).collect()
}

#[test]
fn a_container_lab_runs_where_isoloom_generates_each_form() {
    // Regression: the picker kept its own tables: the helper VM's hypervisors, "no per-machine
    // VMs on UTM", "one VM per machine on any cloud"; none of them asked the lab.
    let o = options(DUAL, Runtime::Docker);
    assert_eq!(o.docker.as_ref().map(|p| p.target.as_str()), Some("docker"));
    // The helper VM: every local hypervisor Isoloom's docker-vm has (Hyper-V isn't one).
    let in_vm = providers(&o.docker_vm);
    assert!(in_vm.contains(&Provider::Utm) && in_vm.contains(&Provider::Virtualbox) && !in_vm.contains(&Provider::Hyperv), "{in_vm:?}");
    // One VM per machine: not on UTM, whose VMs get no network, which says why.
    let per_machine = providers(&o.vms);
    assert!(per_machine.contains(&Provider::Hyperv) && !per_machine.contains(&Provider::Utm), "{per_machine:?}");
    let utm = o.refused.iter().find(|r| r.provider == Some(Provider::Utm) && r.target == "vagrant").expect("UTM's refusal");
    assert!(matches!(&utm.cause, RefusalCause::Isoloom { reason } if reason.contains("no private network")), "{:?}", utm.cause);
    // On a server, the helper VM: ESXi through Vagrant there, Proxmox through its module, each
    // by its own launch path (the picker took ESXi's for `vagrant/vmware_esxi` and Proxmox's
    // for `cloud-vm/proxmox`, neither of which Isoloom has).
    assert_eq!(paths_of(&o.servers), [(Some(Provider::Proxmox), "docker-vm", Some("proxmox")), (Some(Provider::VmwareEsxi), "docker-vm", Some("vmware_esxi"))]);
    assert_eq!(providers(&o.cloud_docker), Provider::CLOUDS);
    assert_eq!(providers(&o.cloud_vm), Provider::CLOUDS);
    assert_eq!(o.cloud_services, [] as [RunPath; 0]);
}

#[test]
fn a_vm_lab_runs_its_vms_where_its_spec_and_isoloom_allow() {
    let o = options(VMS, Runtime::Vm);
    assert!(o.docker.is_none() && o.docker_vm.is_empty() && o.cloud_docker.is_empty());
    assert!(!providers(&o.vms).contains(&Provider::Utm));
    // Proxmox's own target has one form: no provider in its evidence.
    assert_eq!(paths_of(&o.servers), [(Some(Provider::VmwareEsxi), "vagrant", Some("vmware_esxi")), (Some(Provider::Proxmox), "proxmox", None)]);
    // Windows runs on the clouds whose cloud-vm module takes it, and each other says why not.
    assert_eq!(providers(&o.cloud_vm), [Provider::Aws, Provider::Azure]);
    let gcp = o.refused.iter().find(|r| r.provider == Some(Provider::Gcp)).expect("GCP's refusal");
    assert_eq!(gcp.target, "cloud-vm");
    // Its targets leave Proxmox out: ESXi only.
    let o = options(&format!("{VMS}targets: [vagrant]\n"), Runtime::Vm);
    assert_eq!(providers(&o.servers), [Provider::VmwareEsxi]);
    assert_eq!(o.cloud_vm, [] as [RunPath; 0]);
}

/// A Windows machine in the cloud gets no secrets (a lab's inputs, its flags among them): Isoloom
/// sets it up during Terraform's apply, before the runner delivers them. The picker offers it on
/// no cloud, and says why on AWS and Azure, which otherwise take it.
#[test]
fn a_windows_vm_given_a_labs_input_runs_on_no_cloud_and_says_why() {
    let given = VMS.replace("networks: { lan: 10 } }", "networks: { lan: 10 }, files: [{ path: 'C:\\ctf\\flag.txt', input: FLAG }] }");
    let o = options(&format!("{given}inputs: [FLAG]\n"), Runtime::Vm);
    assert_eq!(o.cloud_vm, [] as [RunPath; 0]);
    for cloud in [Provider::Aws, Provider::Azure] {
        let refused = o.refused.iter().find(|r| r.provider == Some(cloud)).unwrap_or_else(|| panic!("{cloud:?}'s refusal"));
        assert_eq!(refused.target, "cloud-vm");
        assert!(matches!(&refused.cause, RefusalCause::Isoloom { reason } if reason.contains("gets `secrets`")), "{:?}", refused.cause);
    }
    // Its VMs still run here, and on ESXi (Vagrant delivers its secrets there).
    assert!(providers(&o.vms).contains(&Provider::Hyperv));
    assert!(providers(&o.servers).contains(&Provider::VmwareEsxi));
}

#[test]
fn each_path_carries_its_own_tier_badge() {
    let tiers = tiers::run_tiers();
    let o = options(DUAL, Runtime::Docker);
    for p in o.docker_vm.iter().chain(&o.vms).chain(&o.servers).chain(&o.cloud_docker).chain(&o.cloud_vm) {
        let tier = tiers.iter().find(|t| t.target == p.target && t.variant == p.variant).unwrap_or_else(|| panic!("{p:?} has an entry"));
        assert_eq!((p.badge, p.evidence.as_str()), (tier.badge, tier.evidence.as_str()), "{p:?}");
    }
}

#[test]
fn cloud_services_run_on_their_own_cloud_only() {
    let spec = "version: 1\nname: goat\ncloud:\n  provider: aws\n  terraform: terraform\n  vars: { region: us-east-1 }\n";
    let o = options(spec, Runtime::Cloud);
    assert_eq!(paths_of(&o.cloud_services), [(Some(Provider::Aws), "cloud-services", Some("aws"))]);
    assert!(o.docker.is_none() && o.vms.is_empty() && o.servers.is_empty() && o.cloud_docker.is_empty() && o.cloud_vm.is_empty());
}

#[test]
fn a_refused_container_form_says_why() {
    // Regression: Isoloom's reason for refusing the lab on this machine's Docker was dropped (a
    // refusal named a provider, and Docker has none): the picker could only say "not supported".
    let broken = DUAL.replace("networks: { lab: 10 }", "networks: { nowhere: 10 }");
    let o = options(&broken, Runtime::Docker);
    assert!(o.docker.is_none());
    let docker = o.refused.iter().find(|r| r.provider.is_none()).expect("Docker's refusal");
    assert_eq!(docker.target, "docker");
    assert!(matches!(&docker.cause, RefusalCause::Isoloom { reason } if !reason.is_empty()), "{:?}", docker.cause);
}

#[test]
fn the_picker_offers_no_way_the_policy_turns_off_and_says_why() {
    // Regression: the policy refused a start only after the click; the picker offered it.
    use crate::platform::policy::{Key, Override};
    let open = options(DUAL, Runtime::Docker);
    assert!(!open.cloud_vm.is_empty() && !open.cloud_docker.is_empty());
    {
        let _policy = Override::with(&[Key::DisableCloudLaunches]);
        let o = options(DUAL, Runtime::Docker);
        assert!(o.cloud_vm.is_empty() && o.cloud_docker.is_empty(), "no cloud account offered");
        assert_eq!(o.docker, open.docker, "this machine still is");
        assert_eq!(o.vms, open.vms);
        let clouds = o.refused.iter().filter(|r| r.cause == by_policy("DisableCloudLaunches")).count();
        assert_eq!(clouds, open.cloud_vm.len() + open.cloud_docker.len(), "each cloud's row says why");
        let _privileged = Override::with(&[Key::RefusePrivilegedLabs]);
        assert_eq!(options(DUAL, Runtime::Docker).docker, open.docker, "a lab without privileged machines runs");
    }
    let privileged = DUAL.replace("networks: { lab: 10 } }", "networks: { lab: 10 }, privileged: true }");
    let _policy = Override::with(&[Key::RefusePrivilegedLabs]);
    let o = options(&privileged, Runtime::Docker);
    assert!(o.docker.is_none() && o.docker_vm.is_empty() && o.vms.is_empty() && o.servers.is_empty() && o.cloud_vm.is_empty(), "nowhere");
    assert!(o.refused.iter().any(|r| r.provider.is_none() && r.cause == by_policy("RefusePrivilegedLabs")), "this machine's Docker says why");
    let utm = o.refused.iter().find(|r| r.provider == Some(Provider::Utm) && r.target == "vagrant");
    assert!(utm.is_some_and(|r| matches!(r.cause, RefusalCause::Isoloom { .. })), "Isoloom's own refusals stay its own: {utm:?}");
}

/// A refusal by this computer's policy `setting`.
fn by_policy(setting: &str) -> RefusalCause {
    RefusalCause::Policy { setting: setting.into() }
}

/// Regression: the webview decided again which hypervisors run a lab built for another CPU, from
/// the catalogue's architectures rather than the spec's machines, and the two could disagree. The
/// run options say it, by the backend's rule: those VMs run here only on an emulator.
#[test]
fn vms_built_for_another_cpu_are_offered_only_on_an_emulator() {
    let native = options_on(VMS, Runtime::Vm, "x86_64");
    assert!(!native.emulated);
    assert!(providers(&native.vms).len() > 1 && providers(&native.vms).contains(&Provider::Virtualbox), "{:?}", providers(&native.vms));
    let emulated = options_on(VMS, Runtime::Vm, "aarch64");
    assert!(emulated.emulated);
    assert_eq!(providers(&emulated.vms), [Provider::Qemu]);
    // A container lab's helper VM is a VM of this machine's CPU: every hypervisor as before.
    let dual = options_on(DUAL, Runtime::Docker, "aarch64");
    assert!(dual.emulated && providers(&dual.vms) == [Provider::Qemu], "its machines as VMs: {:?}", providers(&dual.vms));
    assert_eq!(providers(&dual.docker_vm), providers(&options_on(DUAL, Runtime::Docker, "x86_64").docker_vm));
}
