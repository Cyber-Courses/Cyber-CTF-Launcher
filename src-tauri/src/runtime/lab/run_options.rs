//! Where and in which form a lab can run, as its spec allows: per form, the hypervisors, servers
//! and clouds Isoloom generates it for, each with the launch path it takes there (its Isoloom
//! target and provider or cloud) and that path's maturity badge, and the ones Isoloom refuses,
//! with why. The launch picker offers exactly these; a start checks the same refusals.

use isoloom_spec::model::Target;
use isoloom_targets::Lab;

use crate::runtime::providers::Provider;
use crate::runtime::tiers::{self, Badge};
use crate::runtime::{Runtime, lab};

/// One way to start the lab: the provider or cloud it runs on (none: this machine's Docker), the
/// Isoloom launch path it takes there, and how far that path is proven.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "LabRunPath")]
pub struct RunPath {
    pub provider: Option<Provider>,
    /// The Isoloom target (`docker-vm`, `vagrant`, `cloud-vm`, …).
    pub target: String,
    /// Its provider or cloud as Isoloom's tier evidence names it (`utm`, `aws`), if it has one.
    pub variant: Option<String>,
    /// The path's maturity badge (none when real runs prove it).
    pub badge: Badge,
    /// The evidence behind the badge, for its tooltip.
    pub evidence: String,
}

/// A provider or cloud a form of the lab exists for, which Isoloom refuses the lab on.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "LabRunRefusal")]
pub struct Refusal {
    pub provider: Provider,
    /// The Isoloom target refused there.
    pub target: String,
    pub reason: String,
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
    /// Where a form exists but Isoloom refuses the lab, and why.
    pub refused: Vec<Refusal>,
}

/// Every way lab `lab`, declared `runtime`, can start ([`RunOptions`]).
pub fn run_options(lab: &Lab, runtime: Runtime) -> RunOptions {
    let spec = lab.spec();
    let has = |t: Target| isoloom_spec::effective(&spec).contains(&t);
    let tiers = tiers::run_tiers();
    let path = |target: Target, provider: Option<Provider>| {
        let variant = provider.map(|p| p.id().to_owned());
        let tier = tiers.iter().find(|t| t.target == target.id() && t.variant == variant);
        RunPath {
            provider,
            target: target.id().to_owned(),
            variant,
            badge: tier.map_or(Badge::None, |t| t.badge),
            evidence: tier.map(|t| t.evidence.clone()).unwrap_or_default(),
        }
    };
    let mut out = RunOptions::default();
    // Each of `providers` the lab runs on as `target` (Isoloom's refusal on that provider aside).
    let on = |target: Target, providers: &[Provider], out: &mut Vec<RunPath>, refused: &mut Vec<Refusal>| {
        if !has(target) {
            return;
        }
        for p in providers {
            match lab::refusal_on(lab, target, *p) {
                Some(reason) => refused.push(Refusal { provider: *p, target: target.id().to_owned(), reason }),
                None => out.push(path(target, Some(*p))),
            }
        }
    };
    let local: Vec<Provider> = Provider::ALL.into_iter().filter(|p| !p.is_remote()).collect();
    let docker_vm_local: Vec<Provider> = local.iter().copied().filter(|p| variants(Target::DockerVm).contains(&p.id())).collect();
    let clouds = |target: Target| -> Vec<Provider> { Provider::CLOUDS.into_iter().filter(|c| variants(target).contains(&c.id())).collect() };
    let mut refused = Vec::new();
    match runtime {
        Runtime::Docker => {
            out.docker = (has(Target::Docker) && isoloom_targets::refusal(lab, Target::Docker).is_none()).then(|| path(Target::Docker, None));
            on(Target::DockerVm, &docker_vm_local, &mut out.docker_vm, &mut refused);
            on(Target::Vagrant, &local, &mut out.vms, &mut refused);
            on(Target::DockerVm, &[Provider::VmwareEsxi, Provider::Proxmox], &mut out.servers, &mut refused);
            on(Target::CloudDocker, &clouds(Target::CloudDocker), &mut out.cloud_docker, &mut refused);
            on(Target::CloudVm, &clouds(Target::CloudVm), &mut out.cloud_vm, &mut refused);
        }
        Runtime::Vm => {
            on(Target::Vagrant, &local, &mut out.vms, &mut refused);
            on(Target::Vagrant, &[Provider::VmwareEsxi], &mut out.servers, &mut refused);
            if has(Target::Proxmox) {
                // Proxmox's own target has one form: no provider of its own in the evidence.
                match lab::refusal_on(lab, Target::Proxmox, Provider::Proxmox) {
                    Some(reason) => refused.push(Refusal { provider: Provider::Proxmox, target: Target::Proxmox.id().to_owned(), reason }),
                    None => out.servers.push(RunPath { provider: Some(Provider::Proxmox), ..path(Target::Proxmox, None) }),
                }
            }
            on(Target::CloudVm, &clouds(Target::CloudVm), &mut out.cloud_vm, &mut refused);
        }
        Runtime::Cloud => {
            if let Some(cloud) = spec.cloud.as_ref().map(|c| crate::runtime::cloud_services::provider(c.provider)) {
                on(Target::CloudServices, &[cloud], &mut out.cloud_services, &mut refused);
            }
        }
    }
    out.refused = refused;
    out
}

/// The providers or clouds Isoloom's tier evidence lists for `target`: the ones it generates it
/// for.
fn variants(target: Target) -> Vec<&'static str> {
    isoloom_targets::tiers::variants(target)
}

#[cfg(test)]
mod tests;
