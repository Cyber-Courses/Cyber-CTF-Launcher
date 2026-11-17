//! The launcher on the Isothrum runner: every lab runs from the project Isoloom generates for its
//! target (the folder `.isoloom[-<n>]/docker/`, `.isoloom[-<n>]/vagrant/`, a Terraform module),
//! opened by the runner as a project of its format, through its `Running` operations: a container
//! lab on this machine's Docker, a VM lab on this machine's hypervisor (Vagrant, every local
//! provider) or an ESXi host, and a lab on a Proxmox server (`server`) or in a cloud account
//! (`cloud_runs`). The launcher alone knows the two products: what the lab says of itself (its
//! message, ports, roles, checks) it reads from the lab's spec through Isoloom, and it hands the
//! values a run takes to the runner as environment and secrets, never as files of the runner's.
//!
//! What stays the launcher's: the run's name (`cyberctf-<lab id>`, the Compose project the
//! launcher always used), the runner's home (`isothrum/` in the app data folder, the runner's
//! alone), the words the player reads (the runner's events in `progress`, its error codes in
//! `errors`) and the lab page's shapes (`view`). The attack box is no concern of the run's: the
//! launcher adds it to the lab as one of its machines (`attack_box`), and the run is brought
//! up to date with it ([`Deployment::update`], [`Deployment::retire`]) like with any change of the lab.
//! Everything goes to the home's runner service (`remote`), which this app hosts: an operation,
//! which it runs to its end whatever becomes of the app, a run's record, its consoles. Nothing
//! here opens the runner's store or runs a runner of its own.

mod checks;
mod cloud_runs;
mod deployment;
mod described;
pub(crate) mod errors;
mod esxi;
mod gone;
mod local;
mod old_engine;
#[cfg(feature = "cloud-services")]
mod own_module;
mod progress;
mod remote;
mod server;
mod tunnel;
mod view;
mod web;

pub(crate) use cloud_runs::{CloudPlace, destroy_expired, expired_clouds};
#[cfg(test)]
use deployment::Kind;
pub(crate) use deployment::{Deployment, Form, Site, key_values};
pub(crate) use esxi::EsxiPlace;
pub(crate) use server::ServerPlace;

/// A scripted Terraform `apply`, for the tests that run a module on scripted tools: it leaves its
/// state where it is asked to.
#[cfg(test)]
pub(crate) fn scripted_apply(call: &isothrum_runner::fake::Call) -> isothrum_runner::fake::Reply {
    if let Some(state) = call.args.iter().find_map(|a| a.strip_prefix("-state=")) {
        std::fs::write(state, "{}").unwrap();
    }
    isothrum_runner::fake::Reply::ok("")
}

/// The input variables the Terraform module in `dir` declares, read by the runner from the
/// module itself: nothing runs.
#[cfg(feature = "cloud-services")]
pub(crate) fn module_variables(dir: &Path) -> Result<Vec<isothrum_runner::InputVariable>> {
    isothrum_runner::variables_of(dir).map_err(|e| errors::coded(&e.report()))
}

/// The folder Isoloom generates a lab's files in, at its root (`.isoloom`, `.isoloom-<n>` for an
/// instance).
pub(crate) const OUTPUT_DIR: &str = ".isoloom";
/// The ESXi host's variables Isoloom's Vagrantfiles read.
pub(crate) const ESXI_HOSTNAME: &str = "ESXI_HOSTNAME";
/// The ESXi host's SSH port.
pub(crate) const ESXI_HOSTPORT: &str = "ESXI_HOSTPORT";
/// The variable Isoloom's Compose file reads to publish the lab's services on their own host ports
/// rather than free ones picked at random.
const PUBLISH_FIXED: &str = "ISOLOOM_PUBLISH_FIXED";

use std::path::Path;

use isothrum_runner::{RunName, RunOptions, SecretValue, Secrets};

use super::paths;
use crate::error::{Error, Result};

/// Cancels what the home's runner service runs or has queued on lab `id`'s run, before a stop of
/// the lab.
pub(crate) async fn end_operations(dirs: &crate::platform::AppDirs, id: &str) {
    let (Ok(home), Ok(name)) = (paths::runner_home(dirs), RunName::new(&super::docker::project(id))) else { return };
    remote::cancel_under_way(&home, &name).await;
}

/// The labs whose run has an operation queued or running in the home's runner service: what an
/// update install waits for besides this process's own.
///
/// # Errors
///
/// The service's runs can't be listed.
pub(crate) async fn busy_in_service(dirs: &crate::platform::AppDirs) -> std::result::Result<Vec<String>, String> {
    let home = paths::runner_home(dirs).map_err(|e| e.to_string())?;
    let runs = remote::busy_runs(&home).await?;
    Ok(runs.iter().map(|r| super::docker::lab_of_project(r.as_str()).unwrap_or(r.as_str()).to_owned()).collect())
}

/// Whether lab `id`'s run ended its last operation failed, as the runner records it: its page
/// says the launch failed rather than drawing what is left of it.
pub(crate) async fn last_run_failed(dirs: &crate::platform::AppDirs, id: &str) -> bool {
    record_of(dirs, id).await.ok().flatten().is_some_and(|r| r.state == isothrum_runner::RunState::Failed)
}

/// Lab `id`'s run's operations as the home's runner service journals them, newest first, as JSON
/// (none when no service runs).
///
/// # Errors
///
/// The journal can't be read.
pub(crate) async fn journal_of(dirs: &crate::platform::AppDirs, id: &str) -> Result<serde_json::Value> {
    let home = paths::runner_home(dirs)?;
    let name = RunName::new(&super::docker::project(id)).map_err(|e| errors::coded(&e.report()))?;
    let rows = remote::journal(&home, &name).await.map_err(Error::Invalid)?;
    serde_json::to_value(rows).map_err(|e| Error::Invalid(e.to_string()))
}

/// Follows lab `id`'s run's operations under way in the home's runner service to their end, their
/// lines as `log`'s; nothing when none is under way.
///
/// # Errors
///
/// The service's stream can't be read.
pub(crate) async fn follow_under_way(dirs: &crate::platform::AppDirs, id: &str, log: &mut impl FnMut(String)) -> Result<()> {
    let home = paths::runner_home(dirs)?;
    let name = RunName::new(&super::docker::project(id)).map_err(|e| errors::coded(&e.report()))?;
    remote::follow_under_way(&home, &name, log).await.map_err(Error::Invalid)
}

/// Follows the home's runner service (started when none runs) for as long as its stream lasts,
/// telling `on` what another process asked of it and when a run leaves its standing state.
///
/// # Errors
///
/// No service can be reached, or its stream ended.
pub(crate) async fn follow_others(dirs: &crate::platform::AppDirs, on: &mut impl FnMut(remote::Others)) -> Result<()> {
    let home = paths::runner_home(dirs)?;
    let client = remote::client(&home).await?;
    remote::follow_others(&client, on).await.map_err(Error::Invalid)
}

pub(crate) use remote::{Others, adopt_downloads, downloads, public_key, records, remove_downloads, rows, usage};
#[cfg(test)]
pub(crate) use remote::{stop_service, unreachable as service_unreachable};

/// The address blocks Docker's networks hold now, but lab `id`'s own (left from an earlier run):
/// what its instance must stay clear of, read by `home`'s runner. Empty when Docker can't say.
pub(crate) async fn address_blocks_in_use(home: &Path, id: &str) -> Vec<String> {
    let blocks = match run_name(id) {
        Ok(run) => remote::address_blocks_in_use(home, &run).await,
        Err(e) => Err(e),
    };
    blocks.unwrap_or_else(|e| {
        log::info!("the address blocks Docker's networks hold can't be read: {e}");
        Vec::new()
    })
}

/// Lab `id`'s run's name (`cyberctf-<id>`).
fn run_name(id: &str) -> Result<RunName> {
    RunName::new(&super::docker::project(id)).map_err(|e| errors::coded(&e.report()))
}

/// Forgets lab `id`'s run in the runner's store, once the lab is removed: no run is left behind
/// for a lab that isn't there.
///
/// # Errors
///
/// No service can be reached, or it can't write its store.
pub(crate) async fn forget_run(home: &Path, id: &str) -> Result<()> {
    let Ok(name) = run_name(id) else { return Ok(()) };
    remote::forget_run(home, &name).await
}

/// Lab `id`'s run here, installed in `dir`, opened in the form its record says it was started in
/// (Docker, VMs, or the helper VM an earlier version ran a container lab in): none without a
/// record, or for a run recorded on a server or a cloud, which isn't this machine's to read.
pub(crate) async fn recorded_here(dirs: &crate::platform::AppDirs, dir: &Path, id: &str) -> Result<Option<Deployment>> {
    let Some(record) = record_of(dirs, id).await? else { return Ok(None) };
    let home = paths::runner_home(dirs)?;
    Ok(Some(match super::recorded::RunForm::of(&record) {
        super::recorded::RunForm::Docker => Deployment::docker(&home, dir, id)?,
        super::recorded::RunForm::HelperVm => Deployment::docker_in_vm(&home, dir, id)?,
        super::recorded::RunForm::Vagrant if record.host_label.is_none() => Deployment::vm(&home, dir, id)?,
        _ => return Ok(None),
    }))
}

/// Lab `id`'s run as the runner whose home is the app data folder records it (`cyberctf-<id>`),
/// if it has one: an unreadable record an error.
pub(crate) async fn record_of(dirs: &crate::platform::AppDirs, id: &str) -> Result<Option<isothrum_runner::RunRecord>> {
    // A lab whose id names no run (longer than a run's name may be) never ran.
    let Ok(name) = run_name(id) else { return Ok(None) };
    remote::record(&paths::runner_home(dirs)?, &name).await
}

/// A graphical console's protocol (a run's offer, the runner's console) in the launcher's words:
/// `vnc`, `rdp`, or `none`.
pub(crate) fn protocol_word(protocol: isothrum_runner::ConsoleProtocol) -> &'static str {
    match protocol {
        isothrum_runner::ConsoleProtocol::Vnc => "vnc",
        isothrum_runner::ConsoleProtocol::Rdp => "rdp",
        _ => "none",
    }
}

/// What a start hands the lab: the launch's values for the lab's secret and input slots (a
/// value no slot takes is never delivered), and whether its services keep their own host ports
/// (`CTFd` on 8000) rather than free ones picked at random.
#[derive(Clone, Copy, Default)]
pub(crate) struct Inputs<'a> {
    /// The launch's values, by the name of the lab's slot each goes to.
    pub(crate) values: &'a [(String, String)],
    /// Whether its services keep their own host ports rather than free ones picked at random.
    pub(crate) default_ports: bool,
    /// The Docker context a container lab runs on (`orbstack`, `desktop-linux`…), when the player
    /// chose an engine other than the current one: the runner records it with the run, and every
    /// later operation and read of the run goes there, whatever the current context becomes.
    pub(crate) docker_context: Option<&'a str>,
}

/// The values' names only: a value can be a launch token or a flag.
impl std::fmt::Debug for Inputs<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Inputs")
            .field("names", &self.values.iter().map(|(name, _)| name).collect::<Vec<_>>())
            .field("default_ports", &self.default_ports)
            .field("docker_context", &self.docker_context)
            .finish()
    }
}

#[cfg(test)]
impl<'a> Inputs<'a> {
    /// The launch's `values`, on host ports picked at random.
    pub(crate) fn values(values: &'a [(String, String)]) -> Self {
        Inputs { values, ..Inputs::default() }
    }
}

/// A start's options: the launch's `inputs` (its values as secrets, and fixed host ports when the
/// player chose the lab's own), the hypervisor a VM lab runs on, and a parked run resumed rather
/// than rebuilt.
pub(super) fn start_options(inputs: Inputs<'_>, provider: Option<isothrum_runner::Provider>) -> RunOptions {
    let mut secrets = Secrets::new();
    for (name, value) in inputs.values {
        secrets.insert(name.clone(), SecretValue::new(value.clone()));
    }
    let mut opts = RunOptions::new().with_secrets(secrets);
    if let Some(p) = provider {
        opts = opts.with_provider(p);
    }
    if let Some(context) = inputs.docker_context {
        opts = opts.with_docker_context(context);
    }
    if inputs.default_ports { opts.with_env(PUBLISH_FIXED, "1") } else { opts }
}

/// The runner's provider for the launcher's provider id `id` (`virtualbox`, `libvirt`…): the same
/// names on both sides; one the runner doesn't know is refused rather than dropped, so a start
/// never lands on a hypervisor the player didn't pick.
pub(super) fn runner_provider(id: &str) -> Result<isothrum_runner::Provider> {
    isothrum_runner::from_wire_name(id).ok_or_else(|| Error::Invalid(format!("the runner knows no hypervisor `{id}`")))
}

#[cfg(test)]
mod test_lab;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod vm_tests;
