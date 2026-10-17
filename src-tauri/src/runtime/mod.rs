//! Lab runtimes. A lab is a directory under `<app data>/labs/<lab id>/` with an `isoloom.yml`
//! at its root; the files of the target it runs on are generated under its `.isoloom/` (see
//! `lab`). The UI only ever passes a lab id and a runtime; paths and commands are built here.
//!
//! - `launch`: starting a lab, here or on a server host.
//! - `cloud_services`: cloud-services labs, a Terraform module of the lab's own applied into the
//!   player's cloud account.
//! - `lifecycle`: park, resume, provision and stop; `reaper`: the auto-stop of expired cloud labs.
//! - `status`: what a lab looks like right now, wherever it runs.
//! - `attack_box`, `shell`: the player's attacker beside a lab, and the shells into it.
//! - `commands`: the Tauri commands over all of the above.
//! - `inflight`, `paths`, `remote`: the per-lab lock and in-flight registry, a lab's files and
//!   markers, and SSH to a lab host.
//! - `isothrum`: a container lab on this machine's Docker through the Isothrum runner, behind a
//!   hidden developer switch (`CYBERCTF_RUNNER=isothrum`).

mod attack_box;
mod attack_vm;
mod cloud_services;
mod commands;
mod discover;
pub(crate) use discover::is_lab_id;
mod docker;
mod exegol;
#[cfg(all(test, unix))]
mod fake_clis;
mod inflight;
mod isothrum;
pub mod lab;
mod launch;
mod lifecycle;
mod model;
mod paths;
pub mod providers;
mod proxmox;
#[cfg(test)]
mod real_corpus_labs;
#[cfg(test)]
mod real_docker_vm_labs;
#[cfg(test)]
mod real_flag_labs;
#[cfg(test)]
mod real_labs;
#[cfg(test)]
mod real_server_labs;
#[cfg(test)]
mod real_vm_labs;
mod reaper;
mod remote;
pub mod server;
pub mod server_selftest;
mod shell;
mod ssh;
mod status;
mod terraform;
mod vm;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

// A glob, so each command's generated `__cmd__*` macro is re-exported with it: `lib.rs`
// registers them as `runtime::<command>`.
pub use attack_box::start_attack_vm;
pub use attack_vm::dir as attack_vm_dir;
pub use commands::*;
pub use docker::{PORTS_ENV, attack_network, project as compose_project, subnets_in_use};
pub use exegol::{container as attack_box_container, default_image as default_attack_image};
/// For tests elsewhere that need an operation in flight.
#[cfg(test)]
pub(crate) use inflight::DeployGuard;
pub(crate) use inflight::lock_lab;
pub use inflight::{Action, active_actions, active_deploys, deploying_labs, stopping};
pub use launch::deployed_on_server;
#[cfg(test)]
pub(crate) use launch::start;
pub(crate) use launch::start_locked;
pub use lifecycle::{clear_parked, park_lab, provision_lab, resume_lab, runtime_here, stop_lab};
pub use model::{AttackerAt, Interface, LabStatus, Machine, Network, Output, Park, Place, Port, Service};
pub use paths::{LOCAL_VM_MARKER, validate_id};
pub use reaper::reap_expired_labs;
pub use shell::{ShellKind, attack_vm_shell_target, end_shell_session, end_shell_session_blocking, shell_command_for};
pub use status::{lab_installed, lab_present, primary_url, running_here, vms_recorded_here};
pub use terraform::{PLUGIN_CACHE as TERRAFORM_PLUGINS, clear_plugin_cache};
/// For the generated TypeScript types (`ts_bindings`).
#[cfg(test)]
pub(crate) use {cloud_services::Preview, docker::Check, exegol::ExegolStatus};

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

/// Builders shared by the runtime's unit tests.
#[cfg(test)]
pub(crate) mod test_support {
    use super::{LabStatus, Machine};

    /// A machine in `state`, with nothing else known about it.
    pub(crate) fn machine(name: &str, state: &str) -> Machine {
        Machine {
            name: name.to_string(),
            state: state.to_string(),
            image: String::new(),
            ip: String::new(),
            ports: Vec::new(),
            interfaces: Vec::new(),
            services: Vec::new(),
            infra: false,
        }
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
            attacker: None,
            outputs: Vec::new(),
            message: None,
            reap_error: None,
        }
    }
}
