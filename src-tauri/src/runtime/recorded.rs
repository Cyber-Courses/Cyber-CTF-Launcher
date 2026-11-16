//! Where a started lab runs, in what form, and whether it is parked, as the runner's record of its
//! run (`cyberctf-<id>`) says: the place it was started on (`host_label`, a saved server's or
//! cloud account's id), the project it runs from (Isoloom's folder for its target), and its state
//! and park mode. The one place the launcher reads them from: it keeps no note of its own beside
//! the lab.
//!
//! A run never started, or taken down (`absent`), says nothing: the lab is then read as its
//! catalogue declares it, on this machine.

use isothrum_runner::{RunRecord, RunState};

use super::{Park, Runtime, isothrum, lab};
use crate::error::Result;
use crate::platform::AppDirs;

/// The form a run was started in, as the runner records it: its project's format and folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RunForm {
    /// A Compose project: containers on Docker.
    Docker,
    /// The helper VM an earlier version ran a container lab in (Isoloom's former `docker-vm`).
    HelperVm,
    /// A Vagrant project: VMs.
    Vagrant,
    /// A Terraform module: a server's or a cloud account's.
    Terraform,
}

impl RunForm {
    /// The form of `record`'s run: by its format, and by its folder for the helper VM.
    pub(crate) fn of(record: &isothrum_runner::RunRecord) -> RunForm {
        match record.format {
            isothrum_runner::Format::Compose => RunForm::Docker,
            isothrum_runner::Format::Vagrant if lab::target_folder(&record.project) == Some(lab::DOCKER_VM_FOLDER) => RunForm::HelperVm,
            isothrum_runner::Format::Vagrant => RunForm::Vagrant,
            _ => RunForm::Terraform,
        }
    }
}

/// What the runner records of lab `id`'s run, while something of it stands (any state but
/// absent).
#[derive(Debug, Clone, Default)]
pub(crate) struct Recorded(Option<RunRecord>);

impl Recorded {
    /// Lab `id`'s run as the runner records it. A record that can't be read is an error, never
    /// "nothing recorded": the lab would be read on this machine while it runs on a server.
    pub(crate) fn of(dirs: &AppDirs, id: &str) -> Result<Recorded> {
        Ok(Recorded::from(isothrum::record_of(dirs, id)?))
    }

    /// The saved server or cloud account the run is on, by id; none: this machine.
    pub(crate) fn host(&self) -> Option<&str> {
        self.0.as_ref()?.host_label.as_deref()
    }

    /// The form a lab declared `runtime` runs in: `Vm` for a container lab started as one VM per
    /// machine (here, or as a cloud account's cloud-vm), else its own `runtime`. Every operation
    /// on a started lab follows it, so the webview keeps naming the lab by its catalogue runtime.
    pub(crate) fn form(&self, runtime: Runtime) -> Runtime {
        if runtime == Runtime::Docker && self.per_machine() { Runtime::Vm } else { runtime }
    }

    /// Whether the run is one VM per machine (Isoloom's `vagrant` or `cloud-vm` folder).
    pub(crate) fn per_machine(&self) -> bool {
        self.0.as_ref().is_some_and(|r| matches!(lab::target_folder(&r.project), Some("vagrant" | "cloud-vm")))
    }

    /// Whether the run is the helper VM an earlier version ran a container lab inside.
    pub(crate) fn helper_vm(&self) -> bool {
        self.0.as_ref().is_some_and(|r| RunForm::of(r) == RunForm::HelperVm)
    }

    /// How the run was parked, while it stands parked by a park (not one whose machines were
    /// only found stopped).
    pub(crate) fn parked(&self) -> Option<Park> {
        self.0.as_ref().and_then(parked)
    }

    /// The record itself.
    #[cfg(test)]
    pub(crate) fn record(&self) -> Option<&RunRecord> {
        self.0.as_ref()
    }
}

impl From<Option<RunRecord>> for Recorded {
    fn from(record: Option<RunRecord>) -> Self {
        Recorded(record.filter(|r| r.state != RunState::Absent))
    }
}

/// How `record`'s run was parked, while it stands parked by a park: as that park parked it. None
/// for a run the runner only found stopped (its machines stopped outside the app), which no park
/// put there.
pub(crate) fn parked(record: &RunRecord) -> Option<Park> {
    if record.state != RunState::Parked {
        return None;
    }
    Some(match record.parked? {
        isothrum_runner::Park::Pause => Park::Pause,
        _ => Park::Shutdown,
    })
}

/// The labs whose runs run on this machine, as the runner records them: any state but parked or
/// absent, but a failed run only while a machine of it was last seen running. Read from the
/// home's service when one runs (what it last saw of each run), else from the records. What
/// changed outside the app shows once a status read has seen it.
pub(crate) async fn running_here(dirs: &AppDirs) -> Vec<String> {
    let Ok(home) = super::paths::runner_home(dirs) else { return Vec::new() };
    let runs: Vec<(RunRecord, bool)> = match isothrum::rows(&home).await {
        Ok(Some(rows)) => rows.into_iter().map(|r| (r.record, r.observed.is_some_and(|o| o.machines.iter().any(|m| runs(&m.state))))).collect(),
        Ok(None) => isothrum::records(&home).map(|rs| rs.into_iter().map(|r| (r, false)).collect()).unwrap_or_default(),
        Err(e) => {
            log::warn!("the runner's runs can't be listed: {e}");
            Vec::new()
        }
    };
    let mut ids: Vec<String> = runs
        .into_iter()
        .filter(|(r, seen_running)| r.host_label.is_none() && up(r.state, *seen_running))
        .filter_map(|(r, _)| super::docker::lab_of_project(r.name.as_str()).filter(|id| super::is_lab_id(id)).map(str::to_owned))
        .collect();
    ids.sort();
    ids
}

/// Whether a run in `state` runs: up or moving, or failed with a machine `seen_running`.
fn up(state: RunState, seen_running: bool) -> bool {
    match state {
        RunState::Parked | RunState::Absent => false,
        RunState::Failed => seen_running,
        _ => true,
    }
}

/// Whether a machine in `state` runs (an unhealthy one runs too).
fn runs(state: &isothrum_runner::MachineState) -> bool {
    matches!(state, isothrum_runner::MachineState::Running | isothrum_runner::MachineState::Unhealthy)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(target: &str, state: &str, parked: Option<&str>) -> RunRecord {
        serde_json::from_value(serde_json::json!({
            "version": 1, "name": "cyberctf-t", "project": format!("/home/vagrant/labs/t/.isoloom-2/{target}"),
            "format": if target == "docker" { "compose" } else { "vagrant" }, "state": state, "parked": parked,
        }))
        .unwrap()
    }

    #[test]
    fn the_form_and_park_mode_are_the_records_and_a_run_gone_says_nothing() {
        let docker = Recorded::from(Some(record("docker", "ready", None)));
        assert_eq!((docker.form(Runtime::Docker), docker.parked(), docker.helper_vm()), (Runtime::Docker, None, false));
        // A home folder named after Vagrant's user is no VM form: only Isoloom's own folder counts.
        let vms = Recorded::from(Some(record("vagrant", "parked", Some("pause"))));
        assert_eq!((vms.form(Runtime::Docker), vms.form(Runtime::Cloud), vms.parked()), (Runtime::Vm, Runtime::Cloud, Some(Park::Pause)));
        assert_eq!(Recorded::from(Some(record("vagrant", "parked", None))).parked(), None, "found stopped, not parked by a park");
        assert_eq!(Recorded::from(Some(record("vagrant", "failed", Some("pause")))).parked(), None, "parked no more");
        assert!(Recorded::from(Some(record("docker-vm", "ready", None))).helper_vm());
        let gone = Recorded::from(Some(record("vagrant", "absent", Some("pause"))));
        assert!(gone.record().is_none() && gone.form(Runtime::Docker) == Runtime::Docker && gone.host().is_none());
    }

    #[test]
    fn a_run_runs_while_up_or_moving_and_a_failed_one_while_a_machine_was_seen_running() {
        use super::up;
        for state in [RunState::Ready, RunState::Creating, RunState::Resuming, RunState::Updating, RunState::Parking, RunState::Destroying] {
            assert!(up(state, false), "{state:?}");
        }
        for state in [RunState::Parked, RunState::Absent] {
            assert!(!up(state, true), "{state:?}");
        }
        assert!(up(RunState::Failed, true) && !up(RunState::Failed, false));
    }

    #[tokio::test]
    async fn the_labs_running_here_are_those_the_runner_records_running_on_this_machine() {
        use crate::runtime::test_support::record_run;
        let t = crate::testing::test_app::TestApp::new();
        assert_eq!(super::running_here(&t.dirs()).await, [] as [String; 0]);
        for (lab, host, state) in [
            ("web", None, "ready"),
            ("vms", None, "creating"),
            ("parked", None, "parked"),
            ("gone", None, "absent"),
            ("failed", None, "failed"),
            ("cloud", Some("aws1"), "ready"),
        ] {
            record_run(t.data(), lab, host, "docker", state);
        }
        // No service runs here: the records, a failed run not taken for running.
        assert_eq!(super::running_here(&t.dirs()).await, ["vms", "web"]);
    }
}
