//! Where a lab starts when no place is picked: the app's Start before the player chose, Home's
//! "start again", and a website launch with no target. One rule for all three:
//!
//! 1. Where the lab last ran, as the runner records its run (on a saved server, or here in the
//!    form and on the hypervisor it was started in), while that place is still saved and the
//!    lab's spec still runs there. A container lab an earlier version ran inside a helper VM here
//!    starts in its true form: on Docker when Docker answers, else as one VM per machine on that
//!    hypervisor.
//! 2. Else, for a VM lab, this machine when a hypervisor ready here runs it (an emulator only,
//!    for VMs built for another CPU), the player's preferred one (Settings) first; else the
//!    default server, when the spec runs there.
//! 3. Else this machine (a container lab on Docker).
//!
//! Never a cloud account: it costs money, so it is only ever picked by the player.

use crate::error::Result;
use crate::platform::AppDirs;

use super::lab::{self, RunOptions};
use super::providers::Provider;
use super::{Runtime, isothrum, server};

/// Where a lab starts: on the saved server `host`, or here (no host) on the hypervisor `provider`
/// (none: on Docker), as one VM per machine with `per_machine_vm` (a container lab's VM form).
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "LabStartPlace")]
pub struct StartPlace {
    /// The saved server it starts on, by id; None for this machine.
    pub host: Option<String>,
    /// The hypervisor it starts on here, for a VM lab or a container lab in VMs; None on Docker or
    /// on a server.
    pub provider: Option<Provider>,
    /// A container lab started as one VM per machine.
    pub per_machine_vm: bool,
    /// A VM lab's hypervisor when it starts on this machine, wherever it starts by default (the
    /// player can still pick this machine): the one it last ran on here, else the preferred one,
    /// else the first ready one that runs it. None for a container lab, or when no hypervisor
    /// ready here runs it.
    pub hypervisor: Option<Provider>,
}

/// Where lab `lab_id`, declared `runtime`, starts with no place picked (see the module's rule),
/// from its spec at its `source`; `preferred` is the player's hypervisor of choice.
pub(crate) async fn default_place(dirs: &AppDirs, lab_id: &str, runtime: Runtime, source: lab::Source<'_>, preferred: Option<Provider>) -> Result<StartPlace> {
    // Boxed: the lab's spec is held across the hypervisors' probe, which makes the future large.
    Box::pin(default_place_with(dirs, (lab_id, runtime), source, preferred, (super::providers::ready_here, super::docker::answers))).await
}

/// [`default_place`], the hypervisors ready here read by `ready`, and whether Docker answers here
/// by `docker` (each only when the rule needs it).
async fn default_place_with<F, G>(
    dirs: &AppDirs,
    (lab_id, runtime): (&str, Runtime),
    source: lab::Source<'_>,
    preferred: Option<Provider>,
    (ready, docker): (impl FnOnce() -> F, impl FnOnce() -> G),
) -> Result<StartPlace>
where
    F: Future<Output = Vec<Provider>>,
    G: Future<Output = bool>,
{
    if runtime == Runtime::Cloud {
        return Ok(StartPlace::default());
    }
    let last = isothrum::record_of(dirs, lab_id).await?;
    // A container lab that never ran starts on Docker here: its spec needn't be read.
    if last.is_none() && runtime != Runtime::Vm {
        return Ok(StartPlace::default());
    }
    let lab = lab::lab_at(dirs, lab_id, source).await?;
    let options = lab::run_options(&lab, runtime);
    let last = match last {
        // Docker is asked only of a helper VM's run here, which moves to its true form.
        Some(run) if super::recorded::RunForm::of(&run) == super::recorded::RunForm::HelperVm && run.host_label.is_none() => {
            let docker = docker().await;
            last_place(dirs, run, runtime, &options, docker)?
        }
        Some(run) => last_place(dirs, run, runtime, &options, false)?,
        None => None,
    };
    if runtime != Runtime::Vm {
        return Ok(last.unwrap_or_default());
    }
    // Where it last ran here, else a ready hypervisor here that runs it (VMs built for another
    // CPU are offered only on an emulator), the preferred one first.
    let hypervisor = if let Some(StartPlace { host: None, provider: Some(p), .. }) = last.as_ref() {
        Some(*p)
    } else {
        let runs_here = |p: &Provider| offers(&options.vms, *p);
        let ready: Vec<Provider> = ready().await.into_iter().filter(runs_here).collect();
        super::providers::pick(preferred, &ready)
    };
    if let Some(last) = last {
        return Ok(StartPlace { hypervisor, ..last });
    }
    if let Some(provider) = hypervisor {
        return Ok(StartPlace { provider: Some(provider), hypervisor, ..StartPlace::default() });
    }
    Ok(match server::default_server(dirs)? {
        Some((id, kind)) if offers(&options.servers, kind) => StartPlace { host: Some(id), ..StartPlace::default() },
        _ => StartPlace::default(),
    })
}

/// Where the lab last ran (`run`, the runner's record of it, read from files only: a record that
/// can't be read is the caller's error), while it can start there again: a saved server the spec
/// still runs on, or here in the same form on a hypervisor the spec still runs on; for a helper
/// VM an earlier version ran a container lab inside here, the lab's true form there
/// ([`true_form`], `docker` saying whether Docker answers). None when it can't run there now; an
/// error when the saved servers can't be read (it would start here instead of on its server).
fn last_place(dirs: &AppDirs, run: isothrum_runner::RunRecord, runtime: Runtime, options: &RunOptions, docker: bool) -> Result<Option<StartPlace>> {
    use super::recorded::RunForm as Form;
    if let Some(host) = run.host_label {
        let kind = server::host_provider(dirs, &host)?.filter(|p| !p.is_cloud());
        return Ok(kind.filter(|k| offers(&options.servers, *k)).map(|_| StartPlace { host: Some(host), ..StartPlace::default() }));
    }
    let provider = run.provider.map(|p| isothrum_runner::wire_name(&p)).as_deref().and_then(Provider::from_id);
    Ok(match (Form::of(&run), provider) {
        (Form::Docker, _) if options.docker.is_some() => Some(StartPlace::default()),
        (Form::HelperVm, p) => true_form(options, p, docker),
        (Form::Vagrant, Some(p)) if offers(&options.vms, p) => {
            Some(StartPlace { provider: Some(p), per_machine_vm: runtime == Runtime::Docker, ..StartPlace::default() })
        }
        _ => None,
    })
}

/// Where a container lab an earlier version ran inside a helper VM on hypervisor `provider`
/// starts now, Isoloom generating no helper VM any more: its containers on Docker when Docker
/// answers here (`docker`), else its machines as VMs on that hypervisor, each while the lab's
/// spec runs there; said in the log.
fn true_form(options: &RunOptions, provider: Option<Provider>, docker: bool) -> Option<StartPlace> {
    let place = if docker && options.docker.is_some() {
        Some(StartPlace::default())
    } else {
        provider.filter(|p| offers(&options.vms, *p)).map(|p| StartPlace { provider: Some(p), per_machine_vm: true, ..StartPlace::default() })
    };
    let form = match &place {
        Some(StartPlace { per_machine_vm: true, .. }) => "as one VM per machine",
        Some(_) => "on Docker",
        None => "where it can",
    };
    log::info!("The lab last ran Docker inside a VM, which this version no longer does: it starts {form} instead.");
    place
}

/// Whether `paths` has one on `provider`.
fn offers(paths: &[lab::RunPath], provider: Provider) -> bool {
    paths.iter().any(|p| p.provider == Some(provider))
}

#[cfg(test)]
mod tests;
