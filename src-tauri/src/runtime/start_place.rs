//! Where a lab starts when no place is picked: the app's Start before the player chose, Home's
//! "start again", and a website launch with no target. One rule for all three:
//!
//! 1. Where the lab last ran, as the runner records its run (on a saved server, or here in the
//!    form and on the hypervisor it was started in), while that place is still saved and the
//!    lab's spec still runs there.
//! 2. Else, for a VM lab, this machine when a hypervisor ready here runs it (an emulator only,
//!    for VMs built for another CPU); else the default server, when the spec runs there.
//! 3. Else this machine (a container lab on Docker).
//!
//! Never a cloud account: it costs money, so it is only ever picked by the player.

use crate::error::Result;
use crate::platform::AppDirs;

use super::lab::{self, RunOptions};
use super::providers::Provider;
use super::{Runtime, docker, isothrum, paths, server};

/// Where a lab starts: on the saved server `host`, or here (no host) on the hypervisor `provider`
/// (none: on Docker), as one VM per machine with `per_machine_vm` (a container lab's VM form).
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "LabStartPlace")]
pub struct StartPlace {
    pub host: Option<String>,
    pub provider: Option<Provider>,
    pub per_machine_vm: bool,
}

/// Where lab `lab_id`, declared `runtime`, starts with no place picked (see the module's rule),
/// from its spec at `repository`@`commit`.
pub(crate) async fn default_place(dirs: &AppDirs, lab_id: &str, runtime: Runtime, repository: &str, commit: &str) -> Result<StartPlace> {
    // Boxed: the lab's spec is held across the hypervisors' probe, which makes the future large.
    Box::pin(default_place_with(dirs, (lab_id, runtime), (repository, commit), super::providers::ready_here)).await
}

/// [`default_place`], the hypervisors ready here read by `ready` (only when the rule needs them).
async fn default_place_with<F>(
    dirs: &AppDirs,
    (lab_id, runtime): (&str, Runtime),
    (repository, commit): (&str, &str),
    ready: impl FnOnce() -> F,
) -> Result<StartPlace>
where
    F: Future<Output = Vec<Provider>>,
{
    if runtime == Runtime::Cloud {
        return Ok(StartPlace::default());
    }
    let name = docker::project(lab_id);
    let last = isothrum::records(&paths::runner_home(dirs)?)?.into_iter().find(|r| r.name.as_str() == name);
    // A container lab that never ran starts on Docker here: its spec needn't be read.
    if last.is_none() && runtime != Runtime::Vm {
        return Ok(StartPlace::default());
    }
    let lab = lab::lab_at(dirs, lab_id, repository, commit).await?;
    let options = lab::run_options(&lab, runtime);
    if let Some(place) = last.and_then(|run| last_place(dirs, run, runtime, &options)) {
        return Ok(place);
    }
    if runtime != Runtime::Vm {
        return Ok(StartPlace::default());
    }
    // VMs built for another CPU run here only emulated.
    let foreign = !super::launch::emulated_arches(&lab.spec(), std::env::consts::ARCH).is_empty();
    let runs_here = |p: &Provider| offers(&options.vms, *p) && (!foreign || *p == Provider::Qemu);
    if let Some(provider) = ready().await.into_iter().find(runs_here) {
        return Ok(StartPlace { provider: Some(provider), ..StartPlace::default() });
    }
    Ok(match server::default_server(dirs)? {
        Some((id, kind)) if offers(&options.servers, kind) => StartPlace { host: Some(id), ..StartPlace::default() },
        _ => StartPlace::default(),
    })
}

/// Where the lab last ran (`run`, the runner's record of it, read from files only: a record that
/// can't be read is the caller's error), while it can start there again: a saved server the spec
/// still runs on, or here in the same form on a hypervisor the spec still runs on. None when it
/// can't run there now.
fn last_place(dirs: &AppDirs, run: isothrum_runner::RunRecord, runtime: Runtime, options: &RunOptions) -> Option<StartPlace> {
    use isothrum_runner::isoloom_manifest::Target;
    if let Some(host) = run.host_label {
        let kind = server::host_provider(dirs, &host).filter(|p| !p.is_cloud());
        return kind.filter(|k| offers(&options.servers, *k)).map(|_| StartPlace { host: Some(host), ..StartPlace::default() });
    }
    let provider = run.provider.as_deref().and_then(Provider::from_id);
    match (run.target, provider) {
        (Target::Docker, _) if options.docker.is_some() => Some(StartPlace::default()),
        (Target::DockerVm, Some(p)) if offers(&options.docker_vm, p) => Some(StartPlace { provider: Some(p), ..StartPlace::default() }),
        (Target::Vagrant, Some(p)) if offers(&options.vms, p) => {
            Some(StartPlace { provider: Some(p), per_machine_vm: runtime == Runtime::Docker, ..StartPlace::default() })
        }
        _ => None,
    }
}

/// Whether `paths` has one on `provider`.
fn offers(paths: &[lab::RunPath], provider: Provider) -> bool {
    paths.iter().any(|p| p.provider == Some(provider))
}

#[cfg(test)]
mod tests;
