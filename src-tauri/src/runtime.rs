//! Lab runtimes. A lab is a directory under `<app data>/labs/<lab id>/` with an `isoloom.yml`
//! at its root; the files of the target it runs on are generated under its `.isoloom/` (see
//! `lab`). The UI only ever passes a lab id and a runtime; paths and commands are built here.
//!
//! - `launch`: starting a lab, here or on a server host.
//! - `cloud_services`: cloud-services labs, a Terraform module of the lab's own applied into the
//!   player's cloud account.
//! - `lifecycle`: park, resume, provision and stop; `reaper`: the auto-stop of expired cloud labs.
//! - `status`: what a lab looks like right now, wherever it runs.
//! - `terminal_launch`: opening the OS terminal on the attack box shell the runner builds.
//!
//! The Tauri commands over all of the above are thin adapters in `crate::commands` (lab commands
//! in `commands::labs`, server-host commands in `commands::server`).
//! - `inflight`, `paths`, `ssh`: the per-lab lock and in-flight registry, a lab's files and
//!   markers, and SSH to a lab host.
//! - `isothrum`: every lab run through the Isothrum runner: on this machine (Docker, one VM, or
//!   VMs), on a server, in a cloud account.
//! - `attack_machine`: the learner's attack box, a machine the launcher adds to the lab (through
//!   its overlay, `lab::overlay`) and takes out again, wherever the lab runs.

pub(crate) mod attack_machine;
pub(crate) mod cloud_services;
pub(crate) mod discover;
pub(crate) use discover::is_lab_id;
pub(crate) mod docker;
#[cfg(test)]
#[cfg(unix)]
mod fake_clis;
pub(crate) mod host;
mod inflight;
pub(crate) mod isothrum;
pub mod lab;
pub(crate) mod launch;
pub(crate) mod lifecycle;
mod model;
pub(crate) mod paths;
pub mod providers;
pub(crate) mod proxmox;
mod reaper;
pub mod server;
pub mod server_selftest;
pub(crate) mod ssh;
pub(crate) mod status;
pub(crate) mod terminal_launch;
pub(crate) mod terraform;
pub(crate) mod tiers;
pub(crate) mod vm;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub use docker::{LEGACY_ATTACK_BOX_SUFFIXES, PORTS_ENV, legacy_attack_box_names, project as compose_project, subnets_in_use};
/// For tests elsewhere that need an operation in flight.
#[cfg(test)]
pub(crate) use inflight::DeployGuard;
pub(crate) use inflight::lock_lab;
pub use inflight::{Action, active_actions, active_deploys, deploying_labs, stopping};
pub use isothrum_runner::ShellCommand;
pub use launch::deployed_on_server;
#[cfg(test)]
pub(crate) use launch::start;
pub(crate) use launch::{Destination, start_locked};
pub use lifecycle::{clear_parked, park_lab, provision_lab, resume_lab, runtime_here, stop_lab};
pub use model::{Controls, Interface, LabStatus, Machine, Network, Park, Place, Port, Service};
pub use paths::{LOCAL_VM_MARKER, validate_id};
pub use reaper::reap_expired_labs;
pub use status::{lab_installed, lab_present, primary_url, running_here, vms_recorded_here};
pub use terraform::{PLUGIN_CACHE as TERRAFORM_PLUGINS, clear_plugin_cache};
pub use tiers::RunTier;
/// For the generated TypeScript types (`ts_bindings`).
#[cfg(test)]
pub(crate) use {attack_machine::AttackStatus, cloud_services::Preview, docker::Check};

/// Mirrors `LabRuntime` in CyberBackend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, TS)]
#[serde(rename_all = "UPPERCASE")]
pub enum Runtime {
    Docker,
    Vm,
    /// Cloud services (Isoloom's `cloud-services`): a Terraform module of the lab's own,
    /// applied into the player's cloud account. See `cloud_services`.
    Cloud,
}

/// An installed lab the runtime starts, parks or stops: its folder, its id, and how it runs.
/// These three identify and classify a lab everywhere it is driven, so the launch and lifecycle
/// paths pass them as one value instead of repeating them as separate arguments.
#[derive(Clone, Copy)]
pub(crate) struct LabRef<'a> {
    /// The lab's installed folder.
    pub(crate) dir: &'a std::path::Path,
    /// The lab's id (its folder name).
    pub(crate) id: &'a str,
    /// How the lab runs (container, VM, or cloud services).
    pub(crate) runtime: Runtime,
}

/// Builders shared by the runtime's unit tests.
#[cfg(test)]
pub(crate) mod test_support {
    use super::{LabStatus, Machine};

    /// A machine in `state`, with nothing else known about it.
    pub(crate) fn machine(name: &str, state: &str) -> Machine {
        Machine::named(name.to_string(), state.to_string(), false)
    }

    /// A lab status with these running machines and nothing else set.
    pub(crate) fn status_with(running: bool, machines: &[&str]) -> LabStatus {
        LabStatus {
            running,
            parked: None,
            machines: machines.iter().map(|n| machine(n, "running")).collect(),
            networks: Vec::new(),
            url: None,
            host: None,
            expires_at: None,
            place: None,
            provider: None,
            per_machine: false,
            outputs: Vec::new(),
            message: None,
            reap_error: None,
            controls: crate::runtime::Controls::default(),
        }
    }

    /// A temp lab folder holding `spec` as its `isoloom.yml`, for tests that load a lab from disk.
    pub(crate) fn write_lab(spec: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("cyberctf-lab-{}", rand::random::<u32>()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("isoloom.yml"), spec).unwrap();
        dir
    }
}
