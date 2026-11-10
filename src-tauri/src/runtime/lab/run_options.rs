//! Where and in which form a lab can run, as its spec allows: per form, the hypervisors, servers
//! and clouds Isoloom generates it for, each with the launch path it takes there (its Isoloom
//! target and provider or cloud) and that path's maturity badge, and the ones Isoloom refuses,
//! with why, and the ones this computer's policy turns off. The launch picker offers exactly
//! these; a start checks the same refusals.

use isoloom_spec::model::Target;
use isoloom_targets::Lab;

use super::tiers::{self, Badge};
use crate::platform::policy::{self, Key, Policy};
use crate::runtime::providers::Provider;
use crate::runtime::{Runtime, lab};

/// One way to start the lab: the provider or cloud it runs on (none: this machine's Docker), the
/// Isoloom launch path it takes there, and how far that path is proven.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "LabRunPath")]
pub struct RunPath {
    /// The hypervisor, server or cloud it runs on; None for this machine's Docker.
    pub provider: Option<Provider>,
    /// The Isoloom target (`docker-vm`, `vagrant`, `cloud-vm`, …): how its badge is found, not
    /// the webview's.
    #[serde(skip)]
    #[ts(skip)]
    pub target: String,
    /// Its provider or cloud as Isoloom's tier evidence names it (`utm`, `aws`), if it has one.
    #[serde(skip)]
    #[ts(skip)]
    pub variant: Option<String>,
    /// The path's maturity badge (none when real runs prove it).
    pub badge: Badge,
    /// The evidence behind the badge, for its tooltip.
    pub evidence: String,
}

/// A form of the lab Isoloom refuses or this computer's policy turns off: on a provider or cloud,
/// or (no provider) as containers on this machine's Docker; the picker says why on that place's
/// row, naming who refuses it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "LabRunRefusal")]
pub struct Refusal {
    /// The hypervisor, server or cloud refused; None for this machine's Docker.
    pub provider: Option<Provider>,
    /// The Isoloom target refused there.
    #[serde(skip)]
    #[ts(skip)]
    pub target: String,
    /// Why it is refused.
    pub cause: RefusalCause,
}

/// Who refuses a form of the lab, and why.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, ts_rs::TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
#[ts(rename = "LabRunRefusalCause")]
pub enum RefusalCause {
    /// Isoloom doesn't generate the lab there: its reason, in its words.
    Isoloom {
        /// Isoloom's reason.
        reason: String,
    },
    /// This computer's policy turns it off: the policy switch that does (`policy::Key`'s name).
    Policy {
        /// The name of the policy switch.
        setting: String,
    },
}

impl Refusal {
    /// Isoloom's refusal of `target` on `provider`, for `reason`.
    fn by_isoloom(provider: Option<Provider>, target: String, reason: String) -> Refusal {
        Refusal { provider, target, cause: RefusalCause::Isoloom { reason } }
    }
}

/// Every way the lab can start, by form; empty where the lab has no such form or Isoloom refuses
/// it everywhere.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "LabRunOptions")]
pub struct RunOptions {
    /// Containers on this machine's Docker.
    pub docker: Option<RunPath>,
    /// A container lab in one VM on a local hypervisor (`docker-vm`), by hypervisor.
    pub docker_vm: Vec<RunPath>,
    /// One VM per machine on a local hypervisor (`vagrant`): a VM lab, or a container lab with VM
    /// recipes run as one.
    pub vms: Vec<RunPath>,
    /// The kinds of server (ESXi, Proxmox) it runs on, each by the path it takes there.
    pub servers: Vec<RunPath>,
    /// One cloud VM with its containers (`cloud-docker`), by cloud.
    pub cloud_docker: Vec<RunPath>,
    /// One cloud VM per machine (`cloud-vm`), by cloud.
    pub cloud_vm: Vec<RunPath>,
    /// Its own cloud services (`cloud-services`), on its cloud.
    pub cloud_services: Vec<RunPath>,
    /// Where a form exists but Isoloom refuses the lab, or this computer's policy turns it off,
    /// and why.
    pub refused: Vec<Refusal>,
    /// The lab's VMs are built for another CPU than this machine's (`emulated_arches`): here
    /// they run only emulated, so `vms` holds the emulators alone (QEMU).
    pub emulated: bool,
}

/// Every way lab `lab`, declared `runtime`, can start ([`RunOptions`]), as Isoloom generates it
/// and this computer's policy allows.
pub fn run_options(lab: &Lab, runtime: Runtime) -> RunOptions {
    run_options_on(lab, runtime, std::env::consts::ARCH)
}

/// [`run_options`] on a `host` CPU.
fn run_options_on(lab: &Lab, runtime: Runtime, host: &str) -> RunOptions {
    with_policy(generated(lab, runtime, host), policy::current(), lab::is_privileged(&lab.spec()))
}

/// Every way lab `lab`, declared `runtime`, can start as Isoloom generates it, on a `host` CPU
/// (`std::env::consts::ARCH`'s name): VMs built for another CPU only on an emulator.
fn generated(lab: &Lab, runtime: Runtime, host: &str) -> RunOptions {
    let spec = lab.spec();
    let emulated = !emulated_arches(&spec, host).is_empty();
    let local: Vec<Provider> = Provider::ALL.into_iter().filter(|p| !p.is_remote()).collect();
    let docker_vm_local: Vec<Provider> = local.iter().copied().filter(|p| variants(Target::DockerVm).contains(&p.id())).collect();
    // The lab's own VMs here: on an emulator only, when they are built for another CPU.
    let vm_local: Vec<Provider> = local.iter().copied().filter(|p| !emulated || p.is_emulator()).collect();
    let mut paths = Paths { lab, targets: isoloom_spec::effective(&spec), tiers: tiers::run_tiers(), refused: Vec::new() };
    let mut out = RunOptions { emulated, ..RunOptions::default() };
    match runtime {
        Runtime::Docker => {
            out.docker = paths.has(Target::Docker).then(|| on_docker(lab, paths.path(Target::Docker, None), &mut paths.refused)).flatten();
            paths.on(Target::DockerVm, &docker_vm_local, &mut out.docker_vm);
            paths.on(Target::Vagrant, &vm_local, &mut out.vms);
            paths.on(Target::DockerVm, &Provider::SERVERS, &mut out.servers);
            paths.on(Target::CloudDocker, &clouds(Target::CloudDocker), &mut out.cloud_docker);
            paths.on(Target::CloudVm, &clouds(Target::CloudVm), &mut out.cloud_vm);
        }
        Runtime::Vm => {
            paths.on(Target::Vagrant, &vm_local, &mut out.vms);
            paths.on(Target::Vagrant, &[Provider::VmwareEsxi], &mut out.servers);
            // Proxmox's own target has one form: no provider of its own in the evidence.
            paths.offer(Target::Proxmox, Provider::Proxmox, None, &mut out.servers);
            paths.on(Target::CloudVm, &clouds(Target::CloudVm), &mut out.cloud_vm);
        }
        Runtime::Cloud => {
            if let Some(cloud) = spec.cloud.as_ref().map(|c| crate::runtime::cloud_services::provider(c.provider)) {
                paths.on(Target::CloudServices, &[cloud], &mut out.cloud_services);
            }
        }
    }
    out.refused = paths.refused;
    out
}

/// The clouds Isoloom generates `target` for.
fn clouds(target: Target) -> Vec<Provider> {
    Provider::CLOUDS.into_iter().filter(|c| variants(target).contains(&c.id())).collect()
}

/// Builds a lab's launch paths: the targets its spec generates, each path's badge from Isoloom's
/// tiers, and what Isoloom refuses on a provider (`refused`).
struct Paths<'a> {
    lab: &'a Lab,
    targets: Vec<Target>,
    tiers: Vec<tiers::RunTier>,
    refused: Vec<Refusal>,
}

impl Paths<'_> {
    /// Whether the lab's spec generates `target`.
    fn has(&self, target: Target) -> bool {
        self.targets.contains(&target)
    }

    /// The path of `target` on `provider`, with the badge Isoloom's tiers give it there.
    fn path(&self, target: Target, provider: Option<Provider>) -> RunPath {
        let variant = provider.map(|p| p.id().to_owned());
        let tier = self.tiers.iter().find(|t| t.target == target.id() && t.variant == variant);
        let badge = tiers::badge_or_beta(tier, &format!("{}/{}", target.id(), variant.as_deref().unwrap_or("-")));
        RunPath { provider, target: target.id().to_owned(), variant, badge, evidence: tier.map(|t| t.evidence.clone()).unwrap_or_default() }
    }

    /// Each of `providers` the lab runs on as `target`, into `out` ([`Paths::offer`]).
    fn on(&mut self, target: Target, providers: &[Provider], out: &mut Vec<RunPath>) {
        for p in providers {
            self.offer(target, *p, Some(*p), out);
        }
    }

    /// The lab as `target` on `provider` (its badge read as `tier`'s), into `out`, unless the spec
    /// doesn't generate `target` or Isoloom refuses it there: then its reason goes in `refused`.
    fn offer(&mut self, target: Target, provider: Provider, tier: Option<Provider>, out: &mut Vec<RunPath>) {
        if !self.has(target) {
            return;
        }
        match lab::refusal_on(self.lab, target, provider) {
            Some(reason) => self.refused.push(Refusal::by_isoloom(Some(provider), target.id().to_owned(), reason)),
            None => out.push(RunPath { provider: Some(provider), ..self.path(target, tier) }),
        }
    }
}

/// `options` with the ways `policy` turns off moved to its refusals, naming the setting (what
/// `launch::policy_allows` refuses at the start): every way, for a `privileged` lab under
/// `RefusePrivilegedLabs`; the cloud accounts' under `DisableCloudLaunches`.
fn with_policy(mut options: RunOptions, policy: Policy, privileged: bool) -> RunOptions {
    let refuse = |key: Key, paths: Vec<RunPath>, refused: &mut Vec<Refusal>| {
        let cause = || RefusalCause::Policy { setting: key.name().into() };
        refused.extend(paths.into_iter().map(|p| Refusal { provider: p.provider, target: p.target, cause: cause() }));
    };
    let o = &mut options;
    if privileged && policy.is_on(Key::RefusePrivilegedLabs) {
        let every = o.docker.take().into_iter().chain([&mut o.docker_vm, &mut o.vms, &mut o.servers].into_iter().flat_map(std::mem::take));
        let every: Vec<RunPath> = every.chain([&mut o.cloud_docker, &mut o.cloud_vm, &mut o.cloud_services].into_iter().flat_map(std::mem::take)).collect();
        refuse(Key::RefusePrivilegedLabs, every, &mut o.refused);
    } else if policy.is_on(Key::DisableCloudLaunches) {
        let clouds: Vec<RunPath> = [&mut o.cloud_docker, &mut o.cloud_vm, &mut o.cloud_services].into_iter().flat_map(std::mem::take).collect();
        refuse(Key::DisableCloudLaunches, clouds, &mut o.refused);
    }
    options
}

/// The lab as containers on this machine's Docker, by its launch path `path`, unless Isoloom
/// refuses it there: then its reason goes in `refused`.
fn on_docker(lab: &Lab, path: RunPath, refused: &mut Vec<Refusal>) -> Option<RunPath> {
    match isoloom_targets::refusal(lab, Target::Docker) {
        Some(reason) => {
            refused.push(Refusal::by_isoloom(None, path.target, reason));
            None
        }
        None => Some(path),
    }
}

/// The CPUs (`std::env::consts::ARCH` names) a lab's VMs are built for that differ from `host`,
/// i.e. the ones QEMU has to emulate. Any: the lab's VMs run here only on an emulator.
pub fn emulated_arches(spec: &lab::Spec, host: &str) -> Vec<&'static str> {
    let mut emulated: Vec<&'static str> = spec
        .machines
        .values()
        .filter(|m| m.vm.is_some())
        .map(|m| match m.arch {
            lab::Arch::Amd64 => "x86_64",
            lab::Arch::Arm64 => "aarch64",
        })
        .filter(|a| *a != host)
        .collect();
    emulated.dedup();
    emulated
}

/// The providers or clouds Isoloom's tier evidence lists for `target`: the ones it generates it
/// for.
fn variants(target: Target) -> Vec<&'static str> {
    isoloom_targets::tiers::variants(target)
}

#[cfg(test)]
mod tests;
