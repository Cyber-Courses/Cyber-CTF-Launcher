//! Where and in which form a lab can run, as its spec allows: per form, the hypervisors, servers
//! and clouds Isoloom generates it for, each with the launch path it takes there (its Isoloom
//! target and provider or cloud), and the ones Isoloom refuses,
//! with why, and the ones this computer's policy turns off. The launch picker offers exactly
//! these; a start checks the same refusals.

use isoloom_spec::model::Target;
use isoloom_targets::Lab;

use crate::platform::host::Arch;
use crate::platform::policy::{self, Key, Policy};
use crate::runtime::providers::Provider;
use crate::runtime::{Runtime, lab};

/// One way to start the lab: the provider or cloud it runs on (none: this machine's Docker), and
/// the Isoloom launch path it takes there.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "LabRunPath")]
pub struct RunPath {
    /// The hypervisor, server or cloud it runs on; None for this machine's Docker.
    pub provider: Option<Provider>,
    /// The Isoloom target (`docker`, `vagrant`, `cloud-vm`, …): the launcher's, not the
    /// webview's.
    #[serde(skip)]
    #[ts(skip)]
    pub target: String,
    /// Its provider or cloud as Isoloom names it (`utm`, `aws`), if it has one.
    #[serde(skip)]
    #[ts(skip)]
    pub variant: Option<String>,
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
    /// One VM per machine on a local hypervisor (`vagrant`): a VM lab, or a container lab with VM
    /// recipes run as one.
    pub vms: Vec<RunPath>,
    /// The kinds of server (ESXi, Proxmox) a VM lab runs on, each by the path it takes there (a
    /// container lab has no form on a server: its containers run on Docker or a cloud VM, its VM
    /// recipes here or in a cloud account).
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
    run_options_on(lab, runtime, Arch::HERE)
}

/// [`run_options`] on a `host` CPU.
fn run_options_on(lab: &Lab, runtime: Runtime, host: Arch) -> RunOptions {
    with_policy(generated(lab, runtime, host), policy::current(), lab::is_privileged(&lab.spec()))
}

/// Every way lab `lab`, declared `runtime`, can start as Isoloom generates it, on a `host` CPU:
/// VMs built for another CPU only on an emulator.
fn generated(lab: &Lab, runtime: Runtime, host: Arch) -> RunOptions {
    let spec = lab.spec();
    let emulated = !emulated_arches(&spec, host).is_empty();
    let local: Vec<Provider> = Provider::ALL.into_iter().filter(|p| !p.is_remote()).collect();
    // The lab's own VMs here: on an emulator only, when they are built for another CPU.
    let vm_local: Vec<Provider> = local.iter().copied().filter(|p| !emulated || p.is_emulator()).collect();
    let mut paths = Paths { lab, targets: lab.possible(), refused: Vec::new() };
    let mut out = RunOptions { emulated, ..RunOptions::default() };
    match runtime {
        Runtime::Docker => {
            out.docker = paths.docker();
            out.vms = paths.on(Target::Vagrant, &vm_local);
            out.cloud_docker = paths.on(Target::CloudDocker, &clouds(Target::CloudDocker));
            out.cloud_vm = paths.on(Target::CloudVm, &clouds(Target::CloudVm));
        }
        Runtime::Vm => {
            out.vms = paths.on(Target::Vagrant, &vm_local);
            out.servers = paths.on(Target::Vagrant, &[Provider::VmwareEsxi]);
            // Proxmox's own target has one form: no provider of its own in the evidence.
            out.servers.extend(paths.offer(Target::Proxmox, Provider::Proxmox, None));
            out.cloud_vm = paths.on(Target::CloudVm, &clouds(Target::CloudVm));
        }
        #[cfg(feature = "cloud-services")]
        Runtime::Cloud => {
            if let Some(cloud) = spec.cloud.as_ref().map(|c| crate::runtime::cloud_services::provider(c.provider)) {
                out.cloud_services = paths.on(Target::CloudServices, &[cloud]);
            }
        }
        // Not run by this build: offered nowhere, refused as it says.
        #[cfg(not(feature = "cloud-services"))]
        Runtime::Cloud => paths.refused.push(Refusal::by_isoloom(None, "cloud-services".to_owned(), crate::runtime::CLOUD_UNSUPPORTED.to_owned())),
    }
    out.refused = paths.refused;
    out
}

/// The clouds Isoloom generates `target` for.
fn clouds(target: Target) -> Vec<Provider> {
    Provider::CLOUDS.into_iter().filter(|c| variants(target).contains(&c.id())).collect()
}

/// Builds a lab's launch paths: the targets Isoloom can run it on ([`Lab::possible`]), and what
/// Isoloom refuses on a provider (`refused`).
struct Paths<'a> {
    lab: &'a Lab,
    targets: Vec<Target>,
    refused: Vec<Refusal>,
}

impl Paths<'_> {
    /// Whether the lab's spec generates `target`.
    fn has(&self, target: Target) -> bool {
        self.targets.contains(&target)
    }

    /// The path of `target` on `provider`.
    fn path(target: Target, provider: Option<Provider>) -> RunPath {
        RunPath { provider, target: target.id().to_owned(), variant: provider.map(|p| p.id().to_owned()) }
    }

    /// Each of `providers` the lab runs on as `target` ([`Paths::offer`]).
    fn on(&mut self, target: Target, providers: &[Provider]) -> Vec<RunPath> {
        providers.iter().filter_map(|p| self.offer(target, *p, Some(*p))).collect()
    }

    /// The lab as `target` on `provider` (Isoloom's variant `tier`), unless the spec doesn't
    /// generate `target` or Isoloom refuses it there: then its reason goes in `refused`.
    fn offer(&mut self, target: Target, provider: Provider, tier: Option<Provider>) -> Option<RunPath> {
        if !self.has(target) {
            return None;
        }
        match lab::refusal_on(self.lab, target, provider) {
            Some(reason) => {
                self.refused.push(Refusal::by_isoloom(Some(provider), target.id().to_owned(), reason));
                None
            }
            None => Some(RunPath { provider: Some(provider), ..Paths::path(target, tier) }),
        }
    }

    /// The lab as containers on this machine's Docker, unless the spec doesn't generate them or
    /// Isoloom refuses it there: then its reason goes in `refused`.
    fn docker(&mut self) -> Option<RunPath> {
        if !self.has(Target::Docker) {
            return None;
        }
        let path = Paths::path(Target::Docker, None);
        match super::refused_or_failed(isoloom_targets::refusal(self.lab, Target::Docker)) {
            Some(reason) => {
                self.refused.push(Refusal::by_isoloom(None, path.target, reason));
                None
            }
            None => Some(path),
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
        let every = o.docker.take().into_iter().chain([&mut o.vms, &mut o.servers].into_iter().flat_map(std::mem::take));
        let every: Vec<RunPath> = every.chain([&mut o.cloud_docker, &mut o.cloud_vm, &mut o.cloud_services].into_iter().flat_map(std::mem::take)).collect();
        refuse(Key::RefusePrivilegedLabs, every, &mut o.refused);
    } else if policy.is_on(Key::DisableCloudLaunches) {
        let clouds: Vec<RunPath> = [&mut o.cloud_docker, &mut o.cloud_vm, &mut o.cloud_services].into_iter().flat_map(std::mem::take).collect();
        refuse(Key::DisableCloudLaunches, clouds, &mut o.refused);
    }
    options
}

/// The CPUs a lab's VMs are built for that differ from `host`, i.e. the ones QEMU has to emulate.
/// Any: the lab's VMs run here only on an emulator.
pub fn emulated_arches(spec: &lab::Spec, host: Arch) -> Vec<Arch> {
    let mut emulated: Vec<Arch> = spec
        .machines
        .values()
        .filter(|m| m.vm.is_some())
        .map(|m| match m.arch {
            lab::Arch::Amd64 => Arch::X86_64,
            lab::Arch::Arm64 => Arch::Aarch64,
        })
        .filter(|a| *a != host)
        .collect();
    emulated.dedup();
    emulated
}

/// The providers or clouds Isoloom lists for `target`: the ones it generates it for.
fn variants(target: Target) -> Vec<&'static str> {
    isoloom_targets::validators::variants(target)
}

#[cfg(test)]
mod tests;
