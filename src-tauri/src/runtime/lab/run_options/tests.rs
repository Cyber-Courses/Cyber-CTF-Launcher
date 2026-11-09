use super::*;

/// A lab with both forms: a container per machine, and a VM recipe for it.
const DUAL: &str = "version: 1\nname: t\nnetworks: { lab: { cidr: 10.70.0.0/24 } }\nmachines:\n  web: { os: debian@12, app: { steps: [{ run: web.sh }] }, networks: { lab: 10 } }\n";

/// A VM lab: a Windows machine.
const VMS: &str = "version: 1\nname: ad\nnetworks: { lan: { cidr: 10.20.0.0/24 } }\nmachines:\n  dc: { os: windows-server@2022, networks: { lan: 10 } }\n";

fn options(spec: &str, runtime: Runtime) -> RunOptions {
    run_options(&Lab::new(isoloom_spec::parse(spec).unwrap()), runtime)
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
    let utm = o.refused.iter().find(|r| r.provider == Provider::Utm && r.target == "vagrant").expect("UTM's refusal");
    assert!(utm.reason.contains("no private network"), "{}", utm.reason);
    // On a server, the helper VM: ESXi through Vagrant there, Proxmox through its module, each
    // by its own launch path (the picker took ESXi's for `vagrant/vmware_esxi` and Proxmox's
    // for `cloud-vm/proxmox`, neither of which Isoloom has).
    assert_eq!(paths_of(&o.servers), [(Some(Provider::VmwareEsxi), "docker-vm", Some("vmware_esxi")), (Some(Provider::Proxmox), "docker-vm", Some("proxmox"))]);
    assert_eq!(providers(&o.cloud_docker), Provider::CLOUDS);
    assert_eq!(providers(&o.cloud_vm), Provider::CLOUDS);
    assert!(o.cloud_services.is_empty());
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
    let gcp = o.refused.iter().find(|r| r.provider == Provider::Gcp).expect("GCP's refusal");
    assert_eq!(gcp.target, "cloud-vm");
    // Its targets leave Proxmox out: ESXi only.
    let o = options(&format!("{VMS}targets: [vagrant]\n"), Runtime::Vm);
    assert_eq!(providers(&o.servers), [Provider::VmwareEsxi]);
    assert!(o.cloud_vm.is_empty());
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
