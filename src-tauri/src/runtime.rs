//! Lab runtimes. A lab is a directory under `<app data>/labs/<lab id>/` with an `isoloom.yml`
//! at its root; the files of the target it runs on are generated under its instance's
//! `.isoloom-<n>/` (`.isoloom/` for a lab with no instance; see `lab`). The UI only ever passes a
//! lab id and a runtime; paths and commands are built here.
//!
//! The Tauri commands over these modules are thin adapters in `crate::commands` (lab commands in
//! `commands::labs`, server-host commands in `commands::server`); each module's own doc says what
//! it holds.

pub(crate) mod attack_box;
#[cfg(feature = "cloud-services")]
pub(crate) mod cloud_services;
pub(crate) mod deploy_log;
pub(crate) mod docker;
pub(crate) mod host;
mod inflight;
pub(crate) mod isothrum;
pub mod lab;
pub(crate) mod launch;
pub(crate) mod launch_values;
pub(crate) mod lifecycle;
mod model;
pub(crate) mod paths;
pub mod placement;
pub(crate) mod preflight;
pub mod providers;
pub(crate) mod proxmox;
mod reaper;
pub(crate) mod recorded;
pub mod selftest_report;
pub mod server;
pub mod server_selftest;
pub(crate) mod ssh;
pub(crate) mod status;
pub(crate) mod terminal_launch;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub use docker::lab_of_project;
pub use inflight::Operation;
pub use isothrum_runner::ShellCommand;
#[cfg(test)]
pub(crate) use launch::start;
pub(crate) use launch::{Destination, Placed, check_destination, policy_allows, start_locked};
pub use lifecycle::{clear_parked, park_lab, provision_lab, resume_lab, runtime_here, stop_lab};
pub use model::{Console, Control, Interface, LabStatus, Login, Machine, Network, Park, Place, Port, Preview, Service};
pub(crate) use paths::is_lab_id;
pub use paths::validate_id;
pub use reaper::reap_expired_labs;
pub(crate) use status::{forget_gone, refuse_replace};
pub use status::{lab_installed, lab_present, primary_url};
/// For the generated TypeScript types (`ts_bindings`).
#[cfg(test)]
pub(crate) use {attack_box::AttackStatus, docker::Check};

/// Mirrors `LabRuntime` in CyberBackend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, TS)]
#[serde(rename_all = "UPPERCASE")]
pub enum Runtime {
    /// Containers: the lab's services as Isoloom writes them for Docker (run on this machine's
    /// engine or a cloud VM, or as one VM per machine).
    Docker,
    /// VMs: the lab's machines as Isoloom writes them for Vagrant or a server's or cloud's
    /// Terraform.
    Vm,
    /// Cloud services (Isoloom's `cloud-services`): a Terraform module of the lab's own,
    /// applied into the player's cloud account. See `cloud_services`, built with the
    /// `cloud-services` feature only; without it such a lab starts nowhere
    /// ([`CLOUD_UNSUPPORTED`]).
    Cloud,
}

/// What a lab that runs its own cloud services says in a build without the `cloud-services`
/// feature.
#[cfg_attr(feature = "cloud-services", allow(dead_code, reason = "said only without the feature"))]
pub const CLOUD_UNSUPPORTED: &str = "This lab runs its own cloud services, which this version of Cyber CTF doesn't run.";

impl Runtime {
    /// Its tag as CyberBackend and the webview spell it
    /// (`DOCKER`, `VM`, `CLOUD`): its serde name.
    pub fn tag(self) -> &'static str {
        match self {
            Runtime::Docker => "DOCKER",
            Runtime::Vm => "VM",
            Runtime::Cloud => "CLOUD",
        }
    }
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
    use isothrum_runner::fake::{FakeProcesses, Reply};

    use super::{LabStatus, Machine};

    /// The Azure CLI as testing an Azure account reads it: signed in to the "Pay-As-You-Go"
    /// subscription, with no VM size or usage listed, and Terraform there.
    pub(crate) fn azure_signed_in() -> FakeProcesses {
        FakeProcesses::script()
            .expect("az account show", Reply::ok("Pay-As-You-Go\n"))
            .allow("az vm list-skus", Reply::ok(""))
            .allow("az vm list-usage", Reply::ok(""))
            .allow("terraform version", Reply::ok(""))
            .build()
    }

    /// A node reached over SSH (`ssh … -- user@node 'command'`) answering each command by the
    /// first of `answers` whose text the command contains. Any other call fails the test.
    pub(crate) fn node_over_ssh(answers: Vec<(&'static str, Reply)>) -> FakeProcesses {
        FakeProcesses::model(move |call| {
            let command = call.args.last().map(String::as_str).unwrap_or_default();
            match answers.iter().find(|(text, _)| call.program == "ssh" && command.contains(text)) {
                Some((_, reply)) => reply.clone(),
                None => panic!("unscripted call `{}`", call.line()),
            }
        })
    }

    /// A lab spec with a privileged machine: one `RefusePrivilegedLabs` turns off.
    pub(crate) const PRIVILEGED_LAB: &str = "version: 1\nname: t\nnetworks:\n  lab: { cidr: 10.30.0.0/24 }\nmachines:\n  web:\n    networks: { lab: 10 }\n    privileged: true\n    app: { image: nginx:1.27 }\n";

    /// A machine in `state`, with nothing else known about it.
    pub(crate) fn machine(name: &str, state: &str) -> Machine {
        Machine {
            name: name.into(),
            state: state.into(),
            infra: false,
            ip: String::new(),
            ports: Vec::new(),
            interfaces: Vec::new(),
            services: Vec::new(),
            console: None,
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
            host_id: None,
            expires_at: None,
            place: None,
            provider: None,
            per_machine: false,
            outputs: Vec::new(),
            message: None,
            reap_error: None,
            controls: Vec::new(),
            parks: Vec::new(),
        }
    }

    /// Lab `lab`'s run as the app's runner (its home in the app data folder `home`) records it:
    /// `cyberctf-<lab>`, of `target`, on the place labelled `host` (none: this machine), left in
    /// `state` (`ready`, `parked`, `failed`, `absent`, …) by its last operation.
    pub(crate) fn record_run(home: &std::path::Path, lab: &str, host: Option<&str>, target: &str, state: &str) {
        save_record(home, lab, host, target, &serde_json::json!({ "state": state }));
    }

    /// [`record_run`] of a run on this machine parked by a park in `mode` (`pause`, `shutdown`).
    pub(crate) fn record_parked(home: &std::path::Path, lab: &str, target: &str, mode: &str) {
        save_record(home, lab, None, target, &serde_json::json!({ "state": "parked", "parked": mode }));
    }

    /// [`record_run`] of a VM run on this machine, ready, on the hypervisor `provider` (`qemu`, …).
    pub(crate) fn record_vms_on(home: &std::path::Path, lab: &str, provider: &str) {
        save_record(home, lab, None, "vagrant", &serde_json::json!({ "state": "ready", "provider": provider }));
    }

    /// [`record_run`] with the record's `state` (and `parked`) from `standing`.
    fn save_record(home: &std::path::Path, lab: &str, host: Option<&str>, target: &str, standing: &serde_json::Value) {
        let name = format!("cyberctf-{lab}");
        let driver = if target.starts_with("cloud") || target == "proxmox" {
            "terraform"
        } else if target == "docker" {
            "compose"
        } else {
            "vagrant"
        };
        let mut record = serde_json::json!({
            "version": 0, "name": name, "project": home.join("labs").join(lab).join(".isoloom").join(target),
            "format": driver, "host_label": host
        });
        if let (Some(record), Some(standing)) = (record.as_object_mut(), standing.as_object()) {
            record.extend(standing.clone());
        }
        crate::testing::runner::save_run(&crate::runtime::paths::runner_home_in(home), &record);
    }

    /// A temp lab folder holding `spec` as its `isoloom.yml`, for tests that load a lab from disk;
    /// removed when dropped.
    pub(crate) fn write_lab(spec: &str) -> crate::testing::TempDir {
        let dir = crate::testing::temp_dir("lab");
        std::fs::write(dir.join("isoloom.yml"), spec).unwrap();
        dir
    }
}
