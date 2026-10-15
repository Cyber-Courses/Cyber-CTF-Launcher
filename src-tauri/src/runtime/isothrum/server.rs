//! A VM lab on a Proxmox server through the Isothrum runner: the run manifest Isoloom writes
//! beside its Terraform module (`.isoloom[-<n>]/proxmox/run.json`), behind the same switch as the
//! container labs on this machine ([`super::chosen`]).
//!
//! What stays the launcher's: the run's name (`cyberctf-<lab id>`), the server's profile turned
//! into the module's variables (`lab::host_vars`: the params, the password or API token as the
//! connection slot), the node when the profile names none (the server's first, as the profile's
//! Test connection picks it), the node login SSH goes through first when the lab bridge is
//! internal to the node ([`server::proxmox_jump_for_host`], with the runner's key, which is the
//! launcher's own: both live in the app data folder), and the words the player reads.
//! The runner keeps the Terraform state, the run's slot on the server, and the applied params a
//! destroy replays.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use isothrum_runner::{Lab, RunName, RunOptions, Runner, RunnerConfig, SecretValue, Secrets, SystemProcesses};

use super::{errors, progress, view};
use crate::error::Result;
use crate::runtime::docker::Check;
use crate::runtime::{LabStatus, Place, lab};

/// The module's variables that are the server's secret: connection slots, never params.
const SECRET_VARS: [&str; 2] = ["proxmox_password", "proxmox_api_token"];

/// Where a run goes on its server, as the launcher knows it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ServerPlace {
    /// The server's id among the launcher's hosts (recorded: a later operation elsewhere is
    /// refused).
    pub(crate) label: String,
    /// The server's name, as the lab page shows it.
    pub(crate) name: String,
    /// Its profile's Terraform variables, with its secret (`lab::host_vars`).
    pub(crate) vars: Vec<(String, String)>,
    /// The node when the profile names none: the server's first.
    pub(crate) node: Option<String>,
    /// The node login SSH goes through first (`root@<node>`), when the lab bridge is internal.
    pub(crate) jump: Option<String>,
}

/// One VM lab on a Proxmox server, run by the runner.
pub(crate) struct ServerRun {
    runner: Runner,
    dir: PathBuf,
    name: RunName,
}

impl ServerRun {
    /// Lab `id`, installed in `dir`, on the runner whose home is `home` (the app data folder),
    /// with this host's tools.
    pub(crate) fn new(home: &Path, dir: &Path, id: &str) -> Result<ServerRun> {
        let processes = SystemProcesses::for_host(|var| std::env::var(var).ok(), crate::platform::home_dir().as_deref());
        let config = RunnerConfig::new(home).with_processes(Arc::new(processes));
        let name = RunName::new(&crate::runtime::docker::project(id)).map_err(errors::from_runner)?;
        Ok(ServerRun { runner: Runner::new(config).map_err(errors::from_runner)?, dir: dir.to_path_buf(), name })
    }

    /// The lab's manifest, opened under the run's name.
    fn lab(&self) -> Result<Lab<'_>> {
        self.runner.open(&lab::proxmox_manifest(&self.dir), Some(self.name.clone())).map_err(errors::from_runner)
    }

    /// The runner's SSH public key (the launcher's own), made when there is none: what the node
    /// must let in for a jump.
    pub(crate) async fn public_key(&self) -> Result<String> {
        self.runner.ssh_public_key().await.map_err(errors::from_runner)
    }

    /// Brings the lab up on `place` from its generated files, and says where to start. `env` is
    /// the launch's: the inputs the lab's slots take.
    pub(crate) async fn up(&self, place: &ServerPlace, env: &[(String, String)], log: &mut impl FnMut(String)) -> Result<()> {
        super::up_with(&self.lab()?, &options(place, env), log).await
    }

    /// Removes everything the run created on the server, with the params it was applied with
    /// (the connection is `place`'s, the one that works now).
    pub(crate) async fn down(&self, place: &ServerPlace, log: &mut impl FnMut(String)) -> Result<()> {
        let lab = self.lab()?;
        let (sink, lines) = progress::channel();
        progress::follow(log, lines, lab.down(&options(place, &[]), &sink)).await.map_err(errors::from_runner)
    }

    /// The lab as its page shows it, on the server `place` names.
    pub(crate) async fn status(&self, place: &ServerPlace) -> Result<LabStatus> {
        if !lab::proxmox_manifest(&self.dir).is_file() {
            return Ok(LabStatus { place: Some(Place::Server), ..LabStatus::default() });
        }
        let lab = self.lab()?;
        let status = lab.status().await.map_err(errors::from_runner)?;
        let shown = view::lab_status(lab.manifest(), &status, None);
        Ok(LabStatus { place: Some(Place::Server), host: Some(place.name.clone()), provider: Some("proxmox".into()), ..shown })
    }

    /// Runs the lab's checks over SSH; not available when it declares none.
    pub(crate) async fn check(&self) -> Result<Check> {
        super::checks_of(&self.lab()?).await
    }
}

/// The options of an operation on `place`: its variables as params (its secret as the
/// connection slot), the discovered node when the profile names none, the jump, and the launch's
/// values (`env`) for the lab's input slots.
fn options(place: &ServerPlace, env: &[(String, String)]) -> RunOptions {
    let mut secrets = Secrets::new();
    let mut params = Vec::new();
    for (k, v) in &place.vars {
        if SECRET_VARS.contains(&k.as_str()) {
            secrets.insert(k.clone(), SecretValue::new(v.clone()));
        } else {
            params.push((k.clone(), v.clone()));
        }
    }
    if let Some(node) = place.node.as_ref().filter(|_| !params.iter().any(|(k, _)| k == "node")) {
        params.push(("node".into(), node.clone()));
    }
    for (k, v) in env {
        secrets.insert(k.clone(), SecretValue::new(v.clone()));
    }
    let opts = RunOptions::new().with_params(params).with_secrets(secrets).with_host_label(&place.label);
    match &place.jump {
        Some(jump) => opts.with_ssh_jump(jump),
        None => opts,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn place() -> ServerPlace {
        let vars = [("proxmox_endpoint", "https://10.0.0.5:8006/"), ("proxmox_password", "pw"), ("uplink_bridge", "vmbr1")];
        ServerPlace {
            label: "host-1".into(),
            name: "Lab server".into(),
            vars: vars.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect(),
            node: Some("pve-lab".into()),
            jump: Some("root@10.0.0.5".into()),
        }
    }

    #[test]
    fn the_servers_variables_are_params_and_its_secret_a_slot() {
        let opts = options(&place(), &[("CTF_FLAG".into(), "flag{x}".into())]);
        assert_eq!(opts.params.get("proxmox_endpoint").map(String::as_str), Some("https://10.0.0.5:8006/"));
        assert_eq!(opts.params.get("node").map(String::as_str), Some("pve-lab"), "the discovered node");
        assert!(!opts.params.contains_key("proxmox_password"), "a secret is never a param");
        assert_eq!(opts.secrets.get("proxmox_password").map(SecretValue::expose), Some("pw"));
        assert_eq!(opts.secrets.get("CTF_FLAG").map(SecretValue::expose), Some("flag{x}"));
        assert_eq!((opts.host_label.as_deref(), opts.ssh_jump.as_deref()), (Some("host-1"), Some("root@10.0.0.5")));
    }

    #[test]
    fn the_profiles_own_node_wins_over_the_discovered_one() {
        let mut p = place();
        p.vars.push(("node".into(), "pve2".into()));
        assert_eq!(options(&p, &[]).params.get("node").map(String::as_str), Some("pve2"));
    }
}
