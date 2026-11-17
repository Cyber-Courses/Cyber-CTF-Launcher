//! Where and how a lab runs: one explicit placement, sent with every launch and taken as it is.
//!
//! What can be is Isoloom's to say ([`RunOptions`]: the forms the spec makes possible, on which
//! providers, with this computer's policy applied); what is ready is this machine's (the
//! hypervisors and container engines that answer); what is wanted is the player's: their choice
//! (the Start dialog, the command line's flags, the website's target), else their saved
//! preference. [`choose`] is that one rule, for every caller: the webview asks it through
//! `lab_placement` and sends what it answers, the command line and the website's launches apply
//! it before they ask for the start. A launch never fills anything in or reinterprets it.
//!
//! The rule, in order:
//!
//! 1. A host picked: a cloud account runs the lab in its cloud form there (one VM with its
//!    containers, or one VM per machine); a server (ESXi, Proxmox) runs a VM lab.
//! 2. A cloud-services lab runs only on a cloud account picked for it (it costs money).
//! 3. A hypervisor picked here: the lab's VMs on it (a container lab's one VM per machine).
//! 4. A VM lab, or a container lab asked as one VM per machine: the saved hypervisor when it is
//!    ready and Isoloom runs the lab on it, else the first ready one that does; a VM lab with
//!    none, the saved default server when the lab runs there (unless this machine was picked).
//! 5. Else a container lab's containers here: the engine picked, else the saved one while it
//!    answers (none: the current Docker context); its own host ports when picked, else as saved.
//!
//! Each refusal is the coded error a start says: no hypervisor, no cloud account, a form Isoloom
//! doesn't generate there.

use crate::error::{Code, Error, Result};
use crate::machine::system::docker::Engine;
use crate::platform::AppDirs;
use crate::settings::{PortMode, Settings};

use super::lab::{self, RunOptions, RunPath};
use super::providers::Provider;
use super::{Runtime, server};

/// Where and how a lab runs.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
#[ts(rename = "LabPlacement")]
pub enum Placement {
    /// A container lab's containers on this machine's Docker.
    Docker {
        /// The engine it runs on; none: the current Docker context.
        engine: Option<Engine>,
        /// Its services on their own host ports (`CTFd` on 8000) rather than free ones.
        default_ports: bool,
    },
    /// One VM per machine on this machine's hypervisor: a VM lab, or a container lab's VM form.
    Vms {
        /// The local hypervisor.
        provider: Provider,
    },
    /// A VM lab on a saved server (ESXi, Proxmox).
    Server {
        /// The saved server, by id.
        host: String,
    },
    /// In a saved cloud account.
    Cloud {
        /// The cloud account, by id.
        account: String,
        /// A container lab as one VM per machine (`cloud-vm`) rather than one VM with its
        /// containers (`cloud-docker`). A VM lab is always one VM per machine there.
        per_machine: bool,
    },
}

impl Placement {
    /// The saved server or cloud account it runs on; none for this machine.
    pub(crate) fn host(&self) -> Option<&str> {
        match self {
            Placement::Server { host } => Some(host),
            Placement::Cloud { account, .. } => Some(account),
            Placement::Docker { .. } | Placement::Vms { .. } => None,
        }
    }

    /// The local hypervisor it runs on; none on Docker or on a host.
    pub(crate) fn provider(&self) -> Option<Provider> {
        match self {
            Placement::Vms { provider } => Some(*provider),
            _ => None,
        }
    }

    /// Its services keep their own host ports.
    pub(crate) fn default_ports(&self) -> bool {
        matches!(self, Placement::Docker { default_ports: true, .. })
    }
}

/// What the player picked, each part optional: the rest comes from their saved preference.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Deserialize, ts_rs::TS)]
#[serde(default, rename_all = "camelCase")]
#[ts(rename = "LabPlacementChoice")]
pub struct Choice {
    /// This machine, picked: never a saved server instead.
    pub here: bool,
    /// The saved server or cloud account, by id.
    pub host: Option<String>,
    /// The local hypervisor.
    pub provider: Option<Provider>,
    /// A container lab as one VM per machine (here, or on a cloud account).
    pub per_machine_vm: bool,
    /// The container engine of a container lab here.
    pub engine: Option<Engine>,
    /// A container lab here on its own host ports (`Some(false)`: free ones).
    pub default_ports: Option<bool>,
}

/// What answers on this machine: the hypervisors ready for a lab's VMs (the machine check's), and
/// the container engines running.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Ready {
    /// The local hypervisors ready, in the machine check's order.
    pub hypervisors: Vec<Provider>,
    /// The container engines that answer.
    pub engines: Vec<Engine>,
}

/// The player's saved preference: Settings' hypervisor, engine and ports, and the default server.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Saved {
    /// The hypervisor VM labs run on here.
    pub vm_provider: Option<Provider>,
    /// The container engine a lab here runs on.
    pub docker_engine: Option<Engine>,
    /// How a container lab here gets its host ports.
    pub port_mode: Option<PortMode>,
    /// The default server, and its kind (never a cloud account).
    pub default_server: Option<(String, Provider)>,
}

impl Saved {
    /// The preference in `settings`, with the default server `default_server`.
    pub(crate) fn of(settings: &Settings, default_server: Option<(String, Provider)>) -> Saved {
        Saved { vm_provider: settings.vm_provider, docker_engine: settings.docker_engine, port_mode: settings.port_mode, default_server }
    }
}

/// The lab's VMs run here, on a hypervisor the rule picks (rule 4).
fn picks_a_hypervisor(runtime: Runtime, choice: &Choice) -> bool {
    choice.host.is_none() && choice.provider.is_none() && (runtime == Runtime::Vm || (runtime == Runtime::Docker && choice.per_machine_vm))
}

/// The rule takes a saved engine while it answers (rule 5).
fn checks_the_saved_engine(runtime: Runtime, choice: &Choice, saved: &Saved) -> bool {
    runtime == Runtime::Docker
        && choice.host.is_none()
        && choice.provider.is_none()
        && !choice.per_machine_vm
        && choice.engine.is_none()
        && saved.docker_engine.is_some()
}

/// Where a `runtime` lab runs (see the module's rule): Isoloom's `options` for it, what is
/// `ready` here, the player's `choice` and `saved` preference; `host_kind` names a saved host's
/// kind (none: it isn't saved).
pub fn choose(
    runtime: Runtime,
    options: &RunOptions,
    ready: &Ready,
    choice: &Choice,
    saved: &Saved,
    host_kind: impl Fn(&str) -> Result<Option<Provider>>,
) -> Result<Placement> {
    if let Some(id) = &choice.host {
        let kind = host_kind(id)?.ok_or_else(|| Error::Invalid("that host is no longer set up in the launcher".into()))?;
        return on_host(runtime, options, id, kind, choice.per_machine_vm);
    }
    if runtime == Runtime::Cloud {
        return Err(Error::coded(Code::PickCloudAccount));
    }
    if let Some(provider) = choice.provider {
        return here_on(options, provider);
    }
    if picks_a_hypervisor(runtime, choice) {
        let runs: Vec<Provider> = ready.hypervisors.iter().copied().filter(|p| offers(&options.vms, *p)).collect();
        if let Some(provider) = super::providers::pick(saved.vm_provider, &runs) {
            return Ok(Placement::Vms { provider });
        }
        if runtime == Runtime::Vm
            && !choice.here
            && let Some((host, kind)) = &saved.default_server
            && offers(&options.servers, *kind)
        {
            return Ok(Placement::Server { host: host.clone() });
        }
        return Err(Error::coded(Code::NoHypervisor));
    }
    if options.docker.is_none() {
        return Err(refused(options, None, "containers on this machine"));
    }
    let engine = choice.engine.or(saved.docker_engine.filter(|e| ready.engines.contains(e)));
    let default_ports = choice.default_ports.unwrap_or(saved.port_mode == Some(PortMode::Default));
    Ok(Placement::Docker { engine, default_ports })
}

/// A `runtime` lab on saved host `id` of `kind` (rule 1).
fn on_host(runtime: Runtime, options: &RunOptions, id: &str, kind: Provider, per_machine: bool) -> Result<Placement> {
    if kind.is_cloud() {
        let (paths, form) = match runtime {
            Runtime::Docker if per_machine => (&options.cloud_vm, "one VM per machine"),
            Runtime::Docker => (&options.cloud_docker, "one VM"),
            Runtime::Vm => (&options.cloud_vm, "one VM per machine"),
            Runtime::Cloud => (&options.cloud_services, "its cloud services"),
        };
        if !offers(paths, kind) {
            return Err(refused(options, Some(kind), &format!("{form} on {}", kind.label())));
        }
        return Ok(Placement::Cloud { account: id.to_owned(), per_machine: runtime == Runtime::Vm || (runtime == Runtime::Docker && per_machine) });
    }
    match runtime {
        Runtime::Docker if per_machine => Err(Error::Refused(super::launch::PER_MACHINE_NOT_ON_SERVERS.into())),
        Runtime::Docker => Err(Error::Refused(super::launch::CONTAINERS_NOT_ON_SERVERS.into())),
        Runtime::Cloud => Err(Error::coded(Code::PickCloudAccount)),
        Runtime::Vm if offers(&options.servers, kind) => Ok(Placement::Server { host: id.to_owned() }),
        Runtime::Vm => Err(refused(options, Some(kind), kind.label())),
    }
}

/// The lab's VMs here on `provider`, picked (rule 3).
fn here_on(options: &RunOptions, provider: Provider) -> Result<Placement> {
    if provider.is_cloud() {
        return Err(Error::coded(Code::PickCloudAccount));
    }
    if provider.is_remote() {
        return Err(Error::Refused("pick a server host to run on ESXi or Proxmox".into()));
    }
    if !offers(&options.vms, provider) {
        return Err(refused(options, Some(provider), provider.label()));
    }
    Ok(Placement::Vms { provider })
}

/// Whether `paths` has one on `provider`.
fn offers(paths: &[RunPath], provider: Provider) -> bool {
    paths.iter().any(|p| p.provider == Some(provider))
}

/// The lab refused `there` (on `provider`; none: containers here): Isoloom's or the policy's own
/// refusal when `options` has one, else that the lab has no such form.
fn refused(options: &RunOptions, provider: Option<Provider>, there: &str) -> Error {
    match options.refused.iter().find(|r| r.provider == provider).map(|r| &r.cause) {
        Some(lab::RefusalCause::Policy { setting }) => Error::coded(Code::TurnedOffByPolicy { setting: setting.clone() }),
        Some(lab::RefusalCause::Isoloom { reason }) => Error::Refused(format!("This lab can't run as {there}: {reason}.")),
        None => Error::Refused(format!("This lab doesn't run as {there}.")),
    }
}

/// Where lab `lab_id`, declared `runtime`, runs for `choice` ([`choose`]): Isoloom's options from
/// its spec at `source`, what is ready here (probed only when the rule needs it), and the player's
/// saved settings and default server.
pub(crate) async fn for_lab(dirs: &AppDirs, lab_id: &str, runtime: Runtime, source: lab::Source<'_>, choice: &Choice) -> Result<Placement> {
    let options = options_at(dirs, lab_id, runtime, source).await?;
    let saved = Saved::of(&crate::settings::settings_get(dirs)?, server::default_server(dirs)?);
    let ready = Ready {
        hypervisors: if picks_a_hypervisor(runtime, choice) { super::providers::ready_here().await } else { Vec::new() },
        engines: if checks_the_saved_engine(runtime, choice, &saved) { engines_running().await } else { Vec::new() },
    };
    choose(runtime, &options, &ready, choice, &saved, |id| server::host_provider(dirs, id))
}

/// Every way lab `lab_id`, declared `runtime`, can start, from its spec at `source`
/// ([`lab::run_options`]; a cloud-services lab's from its lab file).
pub(crate) async fn options_at(dirs: &AppDirs, lab_id: &str, runtime: Runtime, source: lab::Source<'_>) -> Result<RunOptions> {
    #[cfg(feature = "cloud-services")]
    if runtime == Runtime::Cloud {
        return super::cloud_services::run_options_at(dirs, lab_id, source).await;
    }
    let lab = lab::lab_at(dirs, lab_id, source).await?;
    Ok(lab::run_options(&lab, runtime))
}

/// The container engines that answer here.
async fn engines_running() -> Vec<Engine> {
    let cx = super::providers::machine();
    isothrum_machine::docker::running_of(&isothrum_machine::docker::engines(&cx).await)
}

#[cfg(test)]
mod tests;
