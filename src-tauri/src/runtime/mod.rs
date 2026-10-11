//! Lab runtimes. A lab is a directory under `<app data>/labs/<lab id>/` with an `isoloom.yml`
//! at its root; the files of the target it runs on are generated under its `.isoloom/` (see
//! `lab`). The UI only ever passes a lab id and a runtime; paths and commands are built here.
//!
//! - `launch`: starting a lab, here or on a server host.
//! - `lifecycle`: park, resume, provision, stop, and the expired-lab reaper.
//! - `status`: what a lab looks like right now, wherever it runs.
//! - `attack_box`, `shell`: the player's attacker beside a lab, and the shells into it.
//! - `commands`: the Tauri commands over all of the above.
//! - `inflight`, `paths`, `remote`: the per-lab lock and in-flight registry, a lab's files and
//!   markers, and SSH to a lab host.

mod attack_box;
mod attack_vm;
mod commands;
mod discover;
mod docker;
mod exegol;
mod inflight;
pub mod lab;
mod launch;
mod lifecycle;
mod model;
mod paths;
pub mod providers;
mod proxmox;
#[cfg(test)]
mod real_labs;
mod registry;
mod remote;
pub mod server;
pub mod server_selftest;
mod shell;
mod ssh;
mod status;
mod terraform;
mod vm;

use serde::{Deserialize, Serialize};

// A glob, so each command's generated `__cmd__*` macro is re-exported with it: `lib.rs`
// registers them as `runtime::<command>`.
pub use attack_box::start_attack_vm;
pub use commands::*;
pub use docker::{PORTS_ENV, subnets_in_use};
pub use inflight::{Action, active_actions, active_deploys, deploying_labs, parking_labs, stopping_labs};
pub use launch::{deployed_on_server, start};
pub use lifecycle::{clear_parked, park_lab, provision_lab, reap_expired_labs, resume_lab, stop_lab};
pub use model::{AttackerAt, Interface, LabStatus, Machine, Network, Park, Place, Port, Service};
pub use paths::{LOCAL_VM_MARKER, validate_id};
pub use shell::{ShellKind, end_shell_session, end_shell_session_blocking, shell_command_for, valid_image};
pub use status::{lab_running_here, primary_url, running_here};

/// Mirrors `LabRuntime` in CyberBackend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Runtime {
    Docker,
    Vm,
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
        }
    }
}
