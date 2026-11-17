//! What Cyber CTF is running on this machine, for the Machine page: each lab whose run the runner
//! records here as running ([`crate::runtime::recorded::runs_here`]), its containers with the
//! memory they use or its VMs on their hypervisor, each stoppable with the lab's own Stop.
//!
//! All of it is the runner's answer, asked of the home's service: what it last saw of each run
//! (no tool runs for it, so a VM's lock is never taken by a poll), and for containers what the
//! engine says they use now (`usage`).

use std::future::Future;
use std::path::Path;

use isothrum_runner::RunName;
use isothrum_runner::store::RunRow;
use serde::Serialize;

use crate::error::{Error, Result};
use crate::platform::AppDirs;
use crate::runtime::Runtime;
use crate::runtime::recorded::{self, RunForm};

/// What stopping a workload needs from the labs' runtime: whether lab `id` is installed and runs
/// here (in a VM or not), and the lab's own stop.
pub trait LabRuntime {
    /// How lab `id` runs here, when it is installed and running on this machine.
    fn runtime_here(&self, id: &str, in_vm: bool) -> impl Future<Output = Option<Runtime>> + Send;

    /// Stops lab `id`, running on `runtime`, as its page would.
    fn stop_lab(&self, id: &str, runtime: Runtime) -> impl Future<Output = Result<()>> + Send;
}

/// A lab running on this machine, as the Machine page lists it: its containers or its VMs.
#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct Workload {
    /// Lab id.
    pub id: String,
    /// What runs it.
    pub kind: WorkloadKind,
    /// Its machines running (containers, or VMs).
    pub count: u32,
    /// Memory used, when the runtime reports it (Docker); 0 if unknown.
    pub mem_bytes: u64,
    /// Hypervisor, for VMs.
    pub provider: Option<String>,
}

/// What runs a [`Workload`]: this machine's Docker, or a hypervisor here.
#[derive(Serialize, serde::Deserialize, Clone, Copy, PartialEq, Eq, Debug, ts_rs::TS)]
#[serde(rename_all = "lowercase")]
pub enum WorkloadKind {
    /// Containers on Docker.
    Docker,
    /// VMs on a hypervisor here.
    Vm,
}

/// The labs running on this machine, as the runner records them: their containers (with the
/// memory they use now) and their VMs.
///
/// # Errors
///
/// The runner's runs can't be listed: the page says so rather than show nothing running.
pub async fn machine_workloads(dirs: AppDirs) -> Result<Vec<Workload>> {
    let home = crate::runtime::paths::runner_home(&dirs)?;
    let mut workloads = Vec::new();
    for (id, row) in recorded::runs_here(&dirs).await? {
        let Some(mut w) = workload(id, &row) else { continue };
        if w.kind == WorkloadKind::Docker {
            w.mem_bytes = memory(&home, &row.record.name).await;
        }
        workloads.push(w);
    }
    Ok(workloads)
}

/// Lab `id`'s workload from its run `row`: its machines last seen running, and the hypervisor its
/// VMs run on. None for a run that is neither containers nor VMs here (a module's).
fn workload(id: String, row: &RunRow) -> Option<Workload> {
    let kind = match RunForm::of(&row.record) {
        RunForm::Docker => WorkloadKind::Docker,
        RunForm::Vagrant => WorkloadKind::Vm,
        RunForm::Terraform => return None,
    };
    let provider = row.record.provider.filter(|_| kind == WorkloadKind::Vm).and_then(provider_id);
    Some(Workload { id, kind, count: u32::try_from(recorded::running_machines(row)).unwrap_or(u32::MAX), mem_bytes: 0, provider })
}

/// `provider` as the webview names hypervisors (`virtualbox`, `hyperv`, ...).
fn provider_id(provider: isothrum_runner::Provider) -> Option<String> {
    serde_json::to_value(provider).ok()?.as_str().map(str::to_owned)
}

/// The memory run `name`'s machines use now, as its engine samples it; 0 when it can't say.
async fn memory(home: &Path, name: &RunName) -> u64 {
    match crate::runtime::isothrum::usage(home, name).await {
        Ok(used) => used.iter().filter_map(|m| m.memory_bytes).fold(0, u64::saturating_add),
        Err(e) => {
            log::warn!("what {name} uses can't be read: {e}");
            0
        }
    }
}

/// Stops a workload from the Machine page: the lab's own Stop (the lab page's: under its lock,
/// with its volumes, pinned ports, networks and attack box or VM). The lab can be started again
/// from its page.
///
/// # Errors
///
/// An id that isn't a lab's, a lab whose run doesn't run here, or the lab's Stop's own.
pub async fn machine_workload_stop(labs: &impl LabRuntime, kind: WorkloadKind, id: String) -> Result<()> {
    if !crate::runtime::is_lab_id(&id) {
        return Err(Error::Invalid(format!("invalid id `{id}`")));
    }
    let Some(runtime) = labs.runtime_here(&id, kind == WorkloadKind::Vm).await else {
        return Err(Error::Invalid(format!("nothing of `{id}` runs on this machine")));
    };
    labs.stop_lab(&id, runtime).await
}

#[cfg(test)]
mod tests;
